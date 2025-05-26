use clap::{
    Command,
    builder::{NonEmptyStringValueParser, ValueParser},
    command, value_parser,
};
use clap_complete::Shell;
use serde::{Deserialize, Serialize};
use std::{fs, io::BufWriter, io::Write, path::PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct ArgumentDef {
    name: String,
    description: String,
    #[serde(default)]
    value_type: Option<String>,
    #[serde(default)]
    possible_values: Vec<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    global: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OptionDef {
    name: String,
    #[serde(default)]
    short_names: Vec<char>,
    #[serde(default)]
    long_names: Vec<String>,
    description: String,
    #[serde(default)]
    value_type: Option<String>,
    #[serde(default)]
    possible_values: Vec<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    global: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CommandDef {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    options: Vec<OptionDef>,
    #[serde(default)]
    subcommands: Vec<CommandDef>,
    #[serde(default)]
    arguments: Vec<ArgumentDef>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CliDef {
    command: CommandDef,
}

fn leak_string<S: AsRef<str>>(s: S) -> &'static str {
    return Box::leak(s.as_ref().to_string().into_boxed_str());
}

fn make_value_parser(settings: (&Vec<String>, &Option<String>)) -> ValueParser {
    let (possible_values, value_type) = settings;

    if !possible_values.is_empty() {
        return possible_values
            .iter()
            .map(|s| leak_string(s))
            .collect::<Vec<_>>()
            .into();
    }

    match value_type.as_deref() {
        Some("file") | Some("dir") | Some("path") => value_parser!(PathBuf),
        Some("boolean") => value_parser!(bool),
        Some("integer") => value_parser!(i64).into(),
        Some("float") => value_parser!(f64).into(),
        _ => NonEmptyStringValueParser::new().into(),
    }
}

fn make_command(def: &CommandDef) -> Command {
    let mut cmd = Command::new(leak_string(&def.name))
        .about(leak_string(&def.description))
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true);

    for option in &def.options {
        cmd = cmd.arg(
            clap::Arg::new(leak_string(&option.name))
                .long(leak_string(&option.name))
                .short_aliases(option.short_names.iter().cloned())
                .aliases(option.long_names.iter().map(|s| leak_string(s)))
                .required(option.required)
                .global(option.global)
                .value_parser(make_value_parser((
                    &option.possible_values,
                    &option.value_type,
                )))
                .help(leak_string(&option.description)),
        );
    }

    for arg in &def.arguments {
        cmd = cmd.arg(
            clap::Arg::new(leak_string(&arg.name))
                .value_parser(make_value_parser((&arg.possible_values, &arg.value_type)))
                .required(arg.required)
                .global(arg.global)
                .help(leak_string(&arg.description)),
        );
    }

    for subcommand in &def.subcommands {
        cmd = cmd.subcommand(make_command(&subcommand));
    }

    return cmd;
}

fn make_cli() -> clap::Command {
    command!()
        .arg(
            clap::Arg::new("input")
                .long("input")
                .short('i')
                .value_hint(clap::ValueHint::FilePath)
                .required(true),
        )
        .arg(
            clap::Arg::new("output")
                .long("output")
                .short('o')
                .value_hint(clap::ValueHint::FilePath)
                .required(true),
        )
    // .arg(
    //     clap::Arg::new("generator")
    //         .long("generator")
    //         .short('g')
    //         .value_parser(value_parser!(Shell))
    //         .ignore_case(true)
    //         .default_value("bash"),
    // )
    // .args_conflicts_with_subcommands(true)
}

fn main() {
    let args = make_cli().get_matches();

    let input = fs::read_to_string(args.get_one::<String>("input").unwrap())
        .expect("Unable to read input file");

    let command_def: CliDef = serde_json::from_str(&input).expect("Unable to parse input JSON");
    let command = make_command(&command_def.command);
    let binary_name = command.get_name().to_string();

    let mut cpp_source = BufWriter::new(Vec::new());
    writeln!(cpp_source, "#include <string>").unwrap();
    writeln!(cpp_source, "#include <vector>").unwrap();
    writeln!(cpp_source, "#include <cstdint>").unwrap();
    writeln!(cpp_source, "#include <map>").unwrap();

    writeln!(cpp_source).unwrap();
    let shells = [
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::PowerShell,
        Shell::Elvish,
    ];

    writeln!(
        cpp_source,
        "#define SHELLS {}",
        shells
            .iter()
            .map(|s| format!("\"{}\"", s.to_string()))
            .collect::<Vec<_>>()
            .join(",")
    )
    .unwrap();

    writeln!(
        cpp_source,
        "const std::map<std::string, std::vector<std::uint8_t>> shell_complete = {{"
    )
    .unwrap();

    for shell in shells {
        let mut buf = BufWriter::new(Vec::new());
        clap_complete::generate(shell, &mut command.clone(), binary_name.clone(), &mut buf);
        buf.flush().unwrap();
        let buf = buf.into_inner().unwrap();

        writeln!(cpp_source, "{{ \"{}\", {{", shell.to_string()).unwrap();

        for (i, byte) in buf.iter().enumerate() {
            if i % 12 == 0 {
                write!(cpp_source, "    ").unwrap();
            }
            write!(cpp_source, "0x{:02X}, ", byte).unwrap();
            if i % 12 == 11 || i == buf.len() - 1 {
                writeln!(cpp_source).unwrap();
            }
        }
        writeln!(cpp_source, "}}}},").unwrap();
    }
    writeln!(cpp_source, "}};").unwrap();

    cpp_source.flush().unwrap();
    let cpp_source = cpp_source.into_inner().unwrap();

    std::fs::write(args.get_one::<String>("output").unwrap(), cpp_source).unwrap();

    // clap_complete::generate(
    //     args.get_one::<Shell>("generator").unwrap().clone(),
    //     &mut command,
    //     binary_name,
    //     &mut output,
    // );
}
