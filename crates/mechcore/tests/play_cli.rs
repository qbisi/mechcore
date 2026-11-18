//! `convert --to html` exports standalone HTML pages, and `play` runs a
//! foreground local HTTP playback engine backed by two-tier caching.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use serde_json::Value;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/marksman-vs-arclight.yaml")
}

fn mechcore(arguments: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(arguments)
        .output()
        .unwrap()
}

fn ok_json(arguments: &[&std::ffi::OsStr]) -> Value {
    let output = mechcore(arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_offline_page(page: &Path, ticks: u64) {
    let html = fs::read_to_string(page).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains(r#""schema":"mechcore.player""#));
    assert!(html.contains(&format!(r#""ticks":{ticks},"#)));
    assert!(!html.contains("{{"));
}

fn embedded_timeline(page: &Path) -> Value {
    let html = fs::read_to_string(page).unwrap();
    let open = r#"<script id="timeline" type="application/json">"#;
    let start = html.find(open).unwrap() + open.len();
    let end = start + html[start..].find("</script>").unwrap();
    serde_json::from_str(&html[start..end]).unwrap()
}

struct LiveServer {
    child: Child,
    report: Value,
}

impl LiveServer {
    fn spawn(arguments: &[&std::ffi::OsStr], cache_dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mechcore"))
            .args(arguments)
            .env("MECHCORE_PLAY_CACHE", cache_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let report: Value = serde_json::from_str(line.trim()).unwrap_or_else(|error| {
            let status = child.wait().unwrap();
            let mut stderr = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            panic!("invalid startup line {line:?} ({error}; status={status}; stderr={stderr})");
        });
        Self { child, report }
    }

    fn url(&self) -> &str {
        self.report["url"].as_str().unwrap()
    }

    fn get(&self, path: &str) -> (u16, String, Vec<u8>) {
        let addr = self
            .url()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/');
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let headers = String::from_utf8_lossy(&raw[..split]).into_owned();
        let body = raw[split + 4..].to_vec();
        let status = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap();
        (status, headers, body)
    }

    fn terminate(mut self) -> std::process::ExitStatus {
        #[cfg(unix)]
        unsafe {
            libc::kill(i32::try_from(self.child.id()).unwrap(), libc::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        self.child.wait().unwrap()
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn create_two_round_match(dir: &Path) -> (PathBuf, PathBuf) {
    let match_path = dir.join("match.yaml");
    let run_str =
        |args: &[&str]| ok_json(&args.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>());
    let path_str = match_path.to_str().unwrap();

    run_str(&["match", "new", path_str, "--seed", "12345", "--map", "1011"]);
    run_str(&["match", "new", path_str]);

    for side in ["blue", "red"] {
        let view = run_str(&["match", "show", path_str, "--side", side]);
        let offer = &view["header"][side]["offers"][0];
        let decision = format!(
            "{{type: choose_advance_team, index: 0, name: {}, specialist: {}}}",
            offer["team"].as_str().unwrap(),
            offer["specialist"].as_str().unwrap()
        );
        run_str(&["match", "act", path_str, "--side", side, &decision]);
        run_str(&["match", "commit", path_str, "--side", side]);
    }

    for _ in 0..2 {
        for side in ["blue", "red"] {
            let view = run_str(&["match", "show", path_str, "--side", side]);
            if let Some(offers) = view["state"]["reinforce_offers"].as_array() {
                let last = offers.len() - 1;
                let name = offers[last].as_str().unwrap();
                let decline =
                    format!("{{type: choose_reinforce_item, index: {last}, name: {name}}}");
                run_str(&["match", "act", path_str, "--side", side, &decline]);
            }
            run_str(&["match", "commit", path_str, "--side", side]);
        }
    }

    let yaml = fs::read_to_string(&match_path).unwrap();
    let tail = yaml.rfind("---\nkind: state").unwrap();
    fs::write(&match_path, &yaml[..tail]).unwrap();

    let grbr_path = dir.join("match.grbr");
    run_str(&[
        "convert",
        path_str,
        "--to",
        "grbr",
        grbr_path.to_str().unwrap(),
    ]);
    (match_path, grbr_path)
}

#[test]
fn convert_to_html_writes_self_contained_pages_for_layout_fight_and_mcfr() {
    let directory = tempfile::tempdir().unwrap();
    let page = directory.path().join("six.html");
    let layout = fixture();
    let report = ok_json(&[
        "convert".as_ref(),
        layout.as_os_str(),
        "--to".as_ref(),
        "html".as_ref(),
        page.as_os_str(),
        "--seed".as_ref(),
        "7".as_ref(),
    ]);
    assert_eq!(report["schema"], "mechcore.convert-html-result");
    assert_eq!(report["from"], "layout");
    assert_eq!(report["to"], "html");
    assert_eq!(report["backend"], "simulator");
    assert_eq!(report["seed"], 7);
    assert_eq!(report["seed_source"], "external");
    assert_eq!(report["output"], page.to_str().unwrap());
    assert_offline_page(&page, report["ticks"].as_u64().unwrap());
    for core in embedded_timeline(&page)["cores"].as_array().unwrap() {
        assert_eq!(core["begin"], 1);
        assert_eq!(core["maximum"], 1);
    }
    let scene = embedded_timeline(&page);
    for team in 0..2 {
        let sum: i64 = scene["score_sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["team"].as_u64() == Some(team))
            .map(|s| s["score"].as_i64().unwrap())
            .sum();
        assert_eq!(
            sum,
            scene["cores"][usize::try_from(1 - team).unwrap()]["damage"]
                .as_i64()
                .unwrap()
        );
    }

    let fight = directory.path().join("fight.yaml");
    ok_json(&[
        "convert".as_ref(),
        layout.as_os_str(),
        "--to".as_ref(),
        "fight".as_ref(),
        fight.as_os_str(),
    ]);
    let from_fight = directory.path().join("fight.html");
    let fight_report = ok_json(&[
        "convert".as_ref(),
        fight.as_os_str(),
        "--to".as_ref(),
        "html".as_ref(),
        from_fight.as_os_str(),
    ]);
    assert_eq!(fight_report["from"], "fight");
    assert_eq!(fight_report["seed_source"], "layout");
    let ticks = fight_report["ticks"].as_u64().unwrap();
    assert_offline_page(&from_fight, ticks);
    for core in embedded_timeline(&from_fight)["cores"].as_array().unwrap() {
        assert!(core["begin"].is_null());
        assert!(core["maximum"].is_null());
    }

    let recording = directory.path().join("fight.mcfr");
    ok_json(&[
        "convert".as_ref(),
        fight.as_os_str(),
        "--to".as_ref(),
        "mcfr".as_ref(),
        recording.as_os_str(),
    ]);
    let from_mcfr = directory.path().join("mcfr.html");
    let mcfr_report = ok_json(&[
        "convert".as_ref(),
        recording.as_os_str(),
        "--to".as_ref(),
        "html".as_ref(),
        from_mcfr.as_os_str(),
    ]);
    assert_eq!(mcfr_report["from"], "mcfr");
    assert_eq!(mcfr_report["ticks"].as_u64().unwrap(), ticks);
    assert!(mcfr_report.get("seed").is_none());
    assert_offline_page(&from_mcfr, ticks);
    assert_eq!(
        embedded_timeline(&from_mcfr),
        embedded_timeline(&from_fight),
        "a fight and the recording of it lay out the same timeline"
    );
}

#[test]
fn live_play_serves_single_fight_over_http_and_reuses_disk_cache() {
    let directory = tempfile::tempdir().unwrap();
    let cache_dir = directory.path().join("play-cache");
    let layout = fixture();

    let server = LiveServer::spawn(
        &[
            "play".as_ref(),
            layout.as_os_str(),
            "--seed".as_ref(),
            "7".as_ref(),
            "--no-open".as_ref(),
        ],
        &cache_dir,
    );
    assert_eq!(server.report["schema"], "mechcore.play-result");
    assert_eq!(server.report["kind"], "layout");
    assert_eq!(server.report["producer"], "simulator");
    assert_eq!(server.report["rounds"], 1);
    assert_eq!(server.report["seed"], 7);
    assert_eq!(server.report["seed_source"], "external");
    assert_eq!(server.report["cached"], false);
    assert_eq!(server.report["opened"], false);
    let ticks = server.report["ticks"].as_u64().unwrap();

    let (status, headers, html_bytes) = server.get("/");
    assert_eq!(status, 200);
    assert!(headers.contains("text/html"));
    let html = String::from_utf8(html_bytes).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains(r#""live":true"#));
    assert!(!html.contains("{{"));

    let (status, _, status_bytes) = server.get("/api/status");
    assert_eq!(status, 200);
    let status_json: Value = serde_json::from_slice(&status_bytes).unwrap();
    assert_eq!(status_json["rounds"][0]["ready"], true);
    assert_eq!(status_json["rounds"][0]["failed"], false);

    let (status, _, round_bytes) = server.get("/api/round/0");
    assert_eq!(status, 200);
    let round_json: Value = serde_json::from_slice(&round_bytes).unwrap();
    assert_eq!(round_json["schema"], "mechcore.player");
    assert_eq!(round_json["ticks"].as_u64().unwrap(), ticks);
    assert_eq!(round_json["cores"][0]["begin"], 1);

    let (missing, _, _) = server.get("/api/round/1");
    assert_eq!(missing, 404);

    let exit = server.terminate();
    assert!(exit.success(), "server terminated with {exit}");

    // Second run with the same MECHCORE_PLAY_CACHE hits the persisted entry.
    let cached = LiveServer::spawn(
        &[
            "play".as_ref(),
            layout.as_os_str(),
            "--seed".as_ref(),
            "7".as_ref(),
            "--no-open".as_ref(),
        ],
        &cache_dir,
    );
    assert_eq!(cached.report["cached"], true);
    assert_eq!(cached.report["seed"], 7);
    assert_eq!(cached.report["seed_source"], "external");
    assert_eq!(cached.report["ticks"].as_u64().unwrap(), ticks);
    let (status, _, cached_round_bytes) = cached.get("/api/round/0");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&cached_round_bytes).unwrap(),
        round_json
    );
    assert!(cached.terminate().success());

    // `--no-cache` bypasses the disk cache even when entries exist.
    let uncached = LiveServer::spawn(
        &[
            "play".as_ref(),
            layout.as_os_str(),
            "--seed".as_ref(),
            "7".as_ref(),
            "--no-open".as_ref(),
            "--no-cache".as_ref(),
        ],
        &cache_dir,
    );
    assert_eq!(uncached.report["cached"], false);
    assert!(uncached.terminate().success());
}

#[test]
fn live_play_keeps_same_round_recording_caches_separate() {
    let directory = tempfile::tempdir().unwrap();
    let cache_dir = directory.path().join("play-cache");
    let other_layout = directory.path().join("rhino.yaml");
    fs::write(
        &other_layout,
        fs::read_to_string(fixture()).unwrap().replace(
            "name: marksman, index: 0, position: {x: 0, y: -50}",
            "name: rhino, index: 0, position: {x: 5, y: -55}",
        ),
    )
    .unwrap();
    let mut timelines = Vec::new();
    for (index, layout) in [fixture(), other_layout].iter().enumerate() {
        let recording = directory.path().join(format!("fight-{index}.mcfr"));
        ok_json(&[
            "convert".as_ref(),
            layout.as_os_str(),
            "--to".as_ref(),
            "mcfr".as_ref(),
            recording.as_os_str(),
            "--seed".as_ref(),
            "7".as_ref(),
        ]);
        let page = directory.path().join(format!("fight-{index}.html"));
        ok_json(&[
            "convert".as_ref(),
            recording.as_os_str(),
            "--to".as_ref(),
            "html".as_ref(),
            page.as_os_str(),
        ]);
        let expected = embedded_timeline(&page);
        assert_eq!(expected["round"], 1);
        for cached in [false, true] {
            let server = LiveServer::spawn(
                &["play".as_ref(), recording.as_os_str(), "--no-open".as_ref()],
                &cache_dir,
            );
            let (status, _, body) = server.get("/api/round/0");
            assert_eq!(status, 200);
            assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), expected);
            assert_eq!(server.report["cached"], cached);
            assert!(server.terminate().success());
        }
        timelines.push(expected);
    }
    assert_ne!(timelines[0], timelines[1]);
}

#[test]
fn live_play_streams_multiround_match_and_grbr_on_demand() {
    let directory = tempfile::tempdir().unwrap();
    let cache_dir = directory.path().join("play-cache");
    let (match_path, grbr_path) = create_two_round_match(directory.path());

    let grbr_server = LiveServer::spawn(
        &["play".as_ref(), grbr_path.as_os_str(), "--no-open".as_ref()],
        &cache_dir,
    );
    assert_eq!(grbr_server.report["kind"], "grbr");
    assert_eq!(grbr_server.report["rounds"], 2);
    assert_eq!(grbr_server.report["seed_source"], "match");
    assert_eq!(grbr_server.report["cached"], false);

    let (status, _, html_bytes) = grbr_server.get("/");
    assert_eq!(status, 200);
    let html = String::from_utf8(html_bytes).unwrap();
    assert!(html.contains(r#""live":true"#));
    assert!(html.contains(r#""index":1,"round":2"#));

    let (status, _, round1_bytes) = grbr_server.get("/api/round/1");
    assert_eq!(status, 200);
    let round1_json: Value = serde_json::from_slice(&round1_bytes).unwrap();
    assert_eq!(round1_json["round"], 2);
    assert!(!round1_json["cores"][0]["begin"].is_null());

    let (status, _, status_bytes) = grbr_server.get("/api/status");
    assert_eq!(status, 200);
    let status_json: Value = serde_json::from_slice(&status_bytes).unwrap();
    assert_eq!(status_json["rounds"][0]["ready"], true);
    assert_eq!(status_json["rounds"][1]["ready"], true);
    assert!(grbr_server.terminate().success());

    let match_server = LiveServer::spawn(
        &[
            "play".as_ref(),
            match_path.as_os_str(),
            "--no-open".as_ref(),
        ],
        &cache_dir,
    );
    assert_eq!(match_server.report["kind"], "match");
    assert_eq!(match_server.report["rounds"], 2);
    assert_eq!(match_server.report["cached"], false);
    let (status, _, match_round1_bytes) = match_server.get("/api/round/1");
    assert_eq!(status, 200);
    let match_round1: Value = serde_json::from_slice(&match_round1_bytes).unwrap();
    assert_eq!(match_round1["round"], 2);
    assert!(match_server.terminate().success());

    let match_cached = LiveServer::spawn(
        &[
            "play".as_ref(),
            match_path.as_os_str(),
            "--no-open".as_ref(),
        ],
        &cache_dir,
    );
    assert_eq!(match_cached.report["cached"], true);
    assert!(match_cached.terminate().success());
}

#[test]
fn play_refuses_invalid_inputs_seeds_and_legacy_page_argument() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state.yaml");
    fs::write(&state, "kind: state\n").unwrap();
    let output = mechcore(&["play".as_ref(), state.as_os_str()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("play does not take a state file"));

    let layout = fixture();
    let html_page = directory.path().join("out.html");
    ok_json(&[
        "convert".as_ref(),
        layout.as_os_str(),
        "--to".as_ref(),
        "html".as_ref(),
        html_page.as_os_str(),
    ]);
    let refused_html = mechcore(&["play".as_ref(), html_page.as_os_str()]);
    assert!(!refused_html.status.success());
    assert!(
        String::from_utf8_lossy(&refused_html.stderr).contains("play does not take a html file")
    );

    let mcfr = directory.path().join("fight.mcfr");
    ok_json(&[
        "convert".as_ref(),
        layout.as_os_str(),
        "--to".as_ref(),
        "mcfr".as_ref(),
        mcfr.as_os_str(),
    ]);
    let refused_seed = mechcore(&[
        "play".as_ref(),
        mcfr.as_os_str(),
        "--seed".as_ref(),
        "3".as_ref(),
    ]);
    assert!(!refused_seed.status.success());
    assert!(String::from_utf8_lossy(&refused_seed.stderr).contains("--seed belongs to a layout"));

    let legacy_page = mechcore(&[
        "play".as_ref(),
        layout.as_os_str(),
        html_page.as_os_str(),
        "--no-open".as_ref(),
    ]);
    assert_eq!(legacy_page.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&legacy_page.stderr);
    assert!(stderr.contains("convert"));
    assert!(stderr.contains("--to html"));
}
