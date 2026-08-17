use std::path::PathBuf;

use momento_common::config_cli::{parse_config_command, ConfigCommand};

#[test]
fn parses_run_and_initialize_commands_in_any_order() {
    assert_eq!(
        parse_config_command(["-c", "/data/config.toml"]).unwrap(),
        ConfigCommand::Run(PathBuf::from("/data/config.toml"))
    );
    assert_eq!(
        parse_config_command(["--init-config", "--config", "/data/config.toml"]).unwrap(),
        ConfigCommand::Initialize(PathBuf::from("/data/config.toml"))
    );
}

#[test]
fn parses_help_without_requiring_a_config_path() {
    assert_eq!(
        parse_config_command(["--help"]).unwrap(),
        ConfigCommand::Help
    );
}

#[test]
fn rejects_missing_duplicate_and_unknown_arguments() {
    for arguments in [
        Vec::<&str>::new(),
        vec!["-c"],
        vec!["-c", "one.toml", "--config", "two.toml"],
        vec!["--unknown"],
    ] {
        assert!(parse_config_command(arguments).is_err());
    }
}
