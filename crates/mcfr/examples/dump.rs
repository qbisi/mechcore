// Scratch: print one tick range of a recording as JSON lines.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = mechcore_mcfr::McfrReader::open(&args[1]).unwrap();
    let from: u32 = args[2].parse().unwrap();
    let to: u32 = args[3].parse().unwrap();
    for tick in from..=to {
        let state = reader.state(tick).unwrap();
        let events = reader.events(tick).unwrap();
        println!(
            "{}",
            serde_json::json!({"tick": tick, "state": state, "events": events})
        );
    }
}
