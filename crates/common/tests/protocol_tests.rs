use common::{parse_command, serialize_response, Command, JiggleConfig, JiggleMode, Response};

#[test]
fn test_default_config_sanitization() {
    let mut config = JiggleConfig {
        enabled: true,
        mode: JiggleMode::Random,
        period_ms: 10, // below min
        scale_px: 120, // above max of 100
    };
    config.sanitize();
    assert_eq!(config.period_ms, 50);
    assert_eq!(config.scale_px, 100);
}

#[test]
fn test_parse_get_and_status() {
    assert_eq!(parse_command("GET").unwrap(), Command::GetStatus);
    assert_eq!(parse_command("status").unwrap(), Command::GetStatus);
    assert_eq!(parse_command("SAVE").unwrap(), Command::SaveConfig);
    assert_eq!(parse_command("RESET").unwrap(), Command::ResetDefaults);
}

#[test]
fn test_parse_set_command() {
    let cmd = parse_command("SET enabled=1 mode=nudge period=2500 scale=8").unwrap();
    if let Command::SetConfig(cfg) = cmd {
        assert!(cfg.enabled);
        assert_eq!(cfg.mode, JiggleMode::GentleNudge);
        assert_eq!(cfg.period_ms, 2500);
        assert_eq!(cfg.scale_px, 8);
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
            scale_px: 10,
        },
        total_jiggles: 42,
        uptime_secs: 120,
    };
    let serialized = serialize_response(&resp);
    assert!(serialized
        .contains("STATUS enabled=1 mode=random period_ms=1500 scale_px=10 jiggles=42 uptime=120"));
}

#[test]
fn test_error_response_formatting() {
    let resp = Response::Error(common::make_string("invalid parameter"));
    let serialized = serialize_response(&resp);
    assert_eq!(serialized.as_str(), "ERR: invalid parameter\r\n");
}
