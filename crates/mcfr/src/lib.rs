//! Shared MCFR data model, canonicalization, Parquet writer, reader, validation,
//! and semantic hashing.
//!
//! Recording producers such as the injected game adapter and the deterministic
//! simulation call [`McfrWriter`] directly. Consumers call [`McfrReader`]
//! directly; MCP orchestration is not part of the file-format boundary.

mod canonical;
mod error;
mod instrumentation;
mod model;
mod parquet_storage;
mod reader;
#[cfg(test)]
mod storage;
mod writer;

pub use error::{Error, Result};
pub use instrumentation::{
    InstrumentationEntry, InstrumentationReader, InstrumentationRecord, InstrumentationSink,
    InstrumentationWriter, NoInstrumentation,
};
pub use model::*;
pub use reader::McfrReader;
pub use writer::McfrWriter;

#[cfg(test)]
mod legacy_oracle_tests {
    use rust_hdf5::H5File;

    use super::*;

    #[test]
    fn hdf5_oracle_and_parquet_decode_the_same_logical_frame() {
        let directory = tempfile::tempdir().unwrap();
        let legacy_path = directory.path().join("oracle.h5");
        let legacy = H5File::create(&legacy_path).unwrap();
        storage::create(&legacy).unwrap();
        let state = WorldSnapshot {
            statuses: vec![StatusState {
                status_id: 1,
                status_type_id: 9,
                source: Some(ObjectRef::new(ObjectKind::Building, 1)),
                target: ObjectRef::new(ObjectKind::Unit, 1),
                additive_stack: 2,
                duration_time: 17,
                max_duration_time: 30,
                step_time: 3,
                step_time_config: 5,
                finished: false,
                frozen: true,
            }],
            ..WorldSnapshot::default()
        };
        let events = TransitionEvents { events: Vec::new() };
        storage::append_tick(&legacy, &state, &events, &[7; 32]).unwrap();
        drop(legacy);
        let legacy = H5File::open(&legacy_path).unwrap();
        let oracle = storage::StorageReader::open(&legacy, 1).unwrap();
        let oracle_state = oracle.state(&legacy, 0).unwrap();
        let oracle_events = oracle.events(&legacy, 0).unwrap();
        assert_eq!(oracle.tick_hash(0).unwrap(), [7; 32]);
        assert_eq!(oracle.tick_hashes(), &[[7; 32]]);

        let output = directory.path().join("battle.mcfr");
        let context = DurableContext {
            game_build: "test-build".into(),
            logic_step: Rational {
                numerator: 1,
                denominator: 20,
            },
            numeric_convention: NumericConvention {
                distance_units_per_meter: 1_000,
                rotation_units_per_degree: 1_000,
                time_units_per_second: 2_000,
            },
            combat_round: 1,
            match_seed: 42,
            identity_contract: IdentityContract::TeamZxSequentialV1,
        };
        let mut writer = McfrWriter::create(&output, &context).unwrap();
        writer.append_tick(state, &events).unwrap();
        writer.finish().unwrap();
        let reader = McfrReader::open(output).unwrap();
        assert_eq!(reader.state(0).unwrap(), oracle_state);
        assert_eq!(reader.events(0).unwrap(), oracle_events);
    }

    #[test]
    fn reader_rejects_decoded_parquet_content_that_does_not_match_tick_hash() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("corrupt.mcfr");
        let context = DurableContext {
            game_build: "test-build".into(),
            logic_step: Rational {
                numerator: 1,
                denominator: 20,
            },
            numeric_convention: NumericConvention {
                distance_units_per_meter: 1_000,
                rotation_units_per_degree: 1_000,
                time_units_per_second: 2_000,
            },
            combat_round: 1,
            match_seed: 42,
            identity_contract: IdentityContract::TeamZxSequentialV1,
        };
        let state = WorldSnapshot::default();
        let events = TransitionEvents { events: Vec::new() };
        let context_bytes = canonical::encode(&context).unwrap();
        let state_bytes = canonical::encode(&state).unwrap();
        let mut scenario = canonical::CanonicalHasher::new("scenario-0.0.1");
        scenario.update(MCFR_FORMAT.as_bytes());
        scenario.update(&context_bytes);
        scenario.update(&state_bytes);
        let scenario = scenario.finalize();
        let wrong_tick_hash = [7; canonical::HASH_BYTES];
        let result = canonical::result_hash(&scenario, &[wrong_tick_hash]);
        let hashes = Hashes::from_raw(scenario, result);

        let mut storage = parquet_storage::StorageWriter::create(directory.path()).unwrap();
        storage
            .append_tick(0, &state, &events, wrong_tick_hash)
            .unwrap();
        let members = storage.finish(&context_bytes, &hashes).unwrap();
        parquet_storage::package_members(members.path(), &output).unwrap();

        let error = McfrReader::open(output).err().unwrap().to_string();
        assert!(error.contains("tick_hash(0)"), "{error}");
    }
}
