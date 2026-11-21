use common::{parse_command, serialize_response, Command, JiggleConfig, JiggleMode, Response};

#[test]
fn test_default_config_sanitization() {
    let mut config = JiggleConfig {
        enabled: true,
        mode: JiggleMode::Random,
        period_ms: 10, // below min 50
        scale_px: 550, // above max 400
        return_to_home: true,
        rth_period_secs: 0, // below min 1
        click_home: true,
    };
    config.sanitize();
    assert_eq!(config.period_ms, 50);
    assert_eq!(config.scale_px, 400);
    assert_eq!(config.rth_period_secs, 1);
}

#[test]
fn test_parse_get_and_status() {
    assert_eq!(parse_command("GET").unwrap(), Command::GetStatus);
    assert_eq!(parse_command("status").unwrap(), Command::GetStatus);
    assert_eq!(parse_command("SAVE").unwrap(), Command::SaveConfig);
    assert_eq!(parse_command("RESET").unwrap(), Command::ResetDefaults);
    assert!(parse_command("").is_err());
    assert!(parse_command("INVALID_CMD").is_err());
}

#[test]
fn test_parse_set_command() {
    let cmd = parse_command(
        "SET enabled=1 mode=circle period=2500 scale=320 rth=1 rth_period=45 click_home=1",
    )
    .unwrap();
    if let Command::SetConfig(cfg) = cmd {
        assert!(cfg.enabled);
        assert_eq!(cfg.mode, JiggleMode::Circle);
        assert_eq!(cfg.period_ms, 2500);
        assert_eq!(cfg.scale_px, 320);
        assert!(cfg.return_to_home);
        assert_eq!(cfg.rth_period_secs, 45);
        assert!(cfg.click_home);
    } else {
        panic!("Expected SetConfig command");
    }

    let cmd2 = parse_command("set enabled=0 mode=nudge period_ms=500 scale_px=15").unwrap();
    if let Command::SetConfig(cfg) = cmd2 {
        assert!(!cfg.enabled);
        assert_eq!(cfg.mode, JiggleMode::GentleNudge);
        assert_eq!(cfg.period_ms, 500);
        assert_eq!(cfg.scale_px, 15);
    } else {
        panic!("Expected SetConfig command");
    }
}

#[test]
fn test_serialize_response() {
    let resp = Response::Status {
        config: JiggleConfig {
            enabled: true,
            mode: JiggleMode::Random,
            period_ms: 1500,
            scale_px: 250,
            return_to_home: true,
            rth_period_secs: 60,
            click_home: true,
        },
        total_jiggles: 42,
        uptime_secs: 120,
        paused: true,
        testing: false,
    };
    let serialized = serialize_response(&resp);
    assert!(serialized.contains(
        "STATUS enabled=1 mode=random period_ms=1500 scale_px=250 rth=1 rth_period=60 click_home=1 jiggles=42 uptime=120 paused=1 testing=0"
    ));

    let ok_serialized = serialize_response(&Response::Ok);
    assert_eq!(ok_serialized.as_str(), "OK\r\n");
}

#[test]
fn test_parse_pause_resume_test() {
    assert_eq!(parse_command("PAUSE").unwrap(), Command::Pause);
    assert_eq!(parse_command("resume").unwrap(), Command::Resume);
    assert_eq!(
        parse_command("TEST 15").unwrap(),
        Command::StartTest { duration_secs: 15 }
    );
    assert_eq!(
        parse_command("test").unwrap(),
        Command::StartTest { duration_secs: 10 }
    );
}

#[test]
fn test_error_response_formatting() {
    let resp = Response::Error(common::make_string("invalid parameter"));
    let serialized = serialize_response(&resp);
    assert_eq!(serialized.as_str(), "ERR: invalid parameter\r\n");
}
