use std::io::Write;
use std::process::Command;

use crate::config;
use crate::commands;
use crate::commands::{ApplicationArguments};
use crate::commands::common::{SourceConfigCommandWrapper, SourceConfigCommand};

#[derive(StructOpt)]
pub struct ShellCommand {
    #[structopt(short = "c", long = "client", help = "select shell (client)", default_value="mysql")]
    pub client: String,
    #[structopt(subcommand)]
    pub source: SourceConfigCommandWrapper,
}


#[cfg(feature = "use_mysql")]
pub fn mysql_client(mysql_config_options: &commands::common::MysqlConfigOptions) {
    let mut cmd = Command::new("mysql");
    if let Some(hostname) =  &mysql_config_options.host {
        cmd.arg("-h").arg(hostname);
    }
    if let Some(username) =  &mysql_config_options.user {
        cmd.arg("-u").arg(username);
    }
    if let Some(port) =  &mysql_config_options.port {
        cmd.arg("-P").arg(port.to_string());
    }

    if let Some(password) = &mysql_config_options.password {
        cmd.arg("-p".to_string() + password);
    }

    if let Some(database) =  &mysql_config_options.database {
        cmd.arg(database);
    }

    cmd
        .status()
        .expect(&format!("failed to execute mysql ({:?})", cmd));
}

#[cfg(feature = "use_mysql")]
pub fn mycli_client(mysql_config_options: &commands::common::MysqlConfigOptions) {
    let mut cmd = Command::new("mycli");
    if let Some(hostname) =  &mysql_config_options.host {
        cmd.arg("-h").arg(hostname);
    }
    if let Some(username) =  &mysql_config_options.user {
        cmd.arg("-u").arg(username);
    }
    if let Some(port) =  &mysql_config_options.port {
        cmd.arg("-P").arg(port.to_string());
    }

    if let Some(password) = &mysql_config_options.password {
        cmd.arg("-p".to_string() + password);
    }

    if let Some(database) =  &mysql_config_options.database {
        cmd.arg(database);
    }

    cmd
        .status()
        .expect(&format!("failed to execute mysql ({:?})", cmd));
}


#[cfg(feature = "use_mysql")]
pub fn mysql_python_client(mysql_config_options: &commands::common::MysqlConfigOptions) {
    config::ensure_config_directory_exists();
    let python_venv_dir = config::get_config_directory().join("python_venv");
    if !python_venv_dir.exists() {
        std::fs::create_dir(&python_venv_dir).unwrap();
    }
    let python_mysql_venv = python_venv_dir.join("mysql");
    if !python_mysql_venv.exists() {
        Command::new("python3")
            .arg("-m")
            .arg("virtualenv")
            .arg("-p")
            .arg("python3")
            .arg(python_mysql_venv.clone())
            .status()
            .expect("creation of virtualenv failed");
        Command::new(python_mysql_venv.join("bin").join("pip"))
            .arg("install")
            .arg("ipython")
            .arg("pymysql")
            .status()
            .expect("could not install dependencies via pip");
    };
    let python_file = python_mysql_venv.join("run.py");
    if !python_file.exists() {
        let content = include_str!("mysql.py");
        std::fs::File::create(&python_file).unwrap().write_all(content.as_ref()).unwrap();
    }

    if let Some(hostname) =  &mysql_config_options.host {
        std::env::set_var("MYSQL_HOST", hostname);
    }
    if let Some(username) =  &mysql_config_options.user {
        std::env::set_var("MYSQL_USER", username);
    }
    if let Some(port) =  &mysql_config_options.port {
        std::env::set_var("MYSQL_PORT", port.to_string());
    }

    if let Some(password) = &mysql_config_options.password {
        std::env::set_var("MYSQL_PASSWORD", password);
    }

    if let Some(database) =  &mysql_config_options.database {
        std::env::set_var("MYSQL_DATABASE", database);
    }


    Command::new(python_mysql_venv.join("bin").join("python"))
        .arg(python_file.clone())
        .status()
        .expect(&format!("could not run python script: {}", python_file.to_str().unwrap()));
}


pub fn shell (_args: &ApplicationArguments, shell_command: &ShellCommand) {

    match &shell_command.source.0 {
        #[cfg(feature = "use_mysql")]
        SourceConfigCommand::Mysql(mysql_config_options) => 
            match shell_command.client.as_ref() {
                "mycli" => mycli_client(&mysql_config_options),
                "mysql" => mysql_client(&mysql_config_options),
                "python" => mysql_python_client(&mysql_config_options),
                _ =>  {
                    eprintln!("unknown client: {}", shell_command.client);
                    std::process::exit(1);
                }
            }
        #[cfg(feature = "use_sqlite")]
        SourceConfigCommand::Sqlite(sqlite_config_options) => {
        },
        #[cfg(feature = "use_postgres")]
        SourceConfigCommand::Postgres(postgres_config_options) => {
        }
    }
}
