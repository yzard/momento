use std::path::PathBuf;

#[derive(Debug, Eq, PartialEq)]
pub enum ConfigCommand {
    Run(PathBuf),
    Initialize(PathBuf),
    Help,
}

pub fn parse_config_command<I, S>(arguments: I) -> Result<ConfigCommand, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut arguments = arguments.into_iter().map(Into::into);
    let mut config_path = None;
    let mut initialize = false;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-c" | "--config" => {
                if config_path.is_some() {
                    return Err("config path was provided more than once".to_string());
                }
                let path = arguments
                    .next()
                    .ok_or_else(|| format!("{argument} requires a config path"))?;
                config_path = Some(PathBuf::from(path));
            }
            "--init-config" => initialize = true,
            "-h" | "--help" => return Ok(ConfigCommand::Help),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }

    let path = config_path.ok_or("missing required argument: -c|--config PATH")?;
    if initialize {
        return Ok(ConfigCommand::Initialize(path));
    }
    Ok(ConfigCommand::Run(path))
}
