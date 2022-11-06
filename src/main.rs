use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use clap::builder::PossibleValuesParser;
use either::{Left, Right};
use encoding::{DecoderTrap, Encoding};
use encoding::all::WINDOWS_1251;
use esl::{ALCH, Field, FileMetadata, FileType, HEDR, NAME, Record, RecordFlags, TES3};
use esl::code::{self, CodePage};
use esl::read::{RecordReadMode, Records};
use filetime::{FileTime, set_file_mtime};
use ini::Ini;
use std::collections::HashMap;
use std::env::current_exe;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufWriter, Read};
use std::mem::transmute;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

/*
fn parse_args() -> (Options, Vec<Option<PathBuf>>) {
    let args = Command::new("ESP Assembler/Disassembler")
        .version(env!("CARGO_PKG_VERSION"))
        .disable_colored_help(true)
        .help_template("Usage: {usage}\n{about}\n\n{options}\n\n{after-help}")
        .after_help("<COND> can be in one of the following form: RECORD_TAG, RECORD_TAG>:FIELD_TAG, or :FIELD_TAG.\n\n\
            When FILE is -, read standard input.\n\n\
            Report bugs to <internalmike@gmail.com> (in English or Russian).\
        ")
        .about("Convert FILEs from the .esm/.esp/.ess format to YAML and back.")
        .disable_help_flag(true)
        .arg(Arg::new("help")
            .short('h')
            .long("help")
            .help("display this help and exit")
            .action(ArgAction::Help)
        )
        .arg(Arg::new("FILE")
            .action(ArgAction::Append)
            .value_parser(value_parser!(OsString))
        )
        .arg(Arg::new("disassemble")
            .short('d')
            .long("disassemble")
            .help("convert binary .es{s,p,m} file to YAML")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("verbose")
            .short('v')
            .long("verbose")
            .help("verbose mode")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("fit")
            .short('f')
            .long("fit")
            .help("remove redundant trailing zeros and other garbage")
            .action(ArgAction::SetTrue)
        )
        .disable_version_flag(true)
        .arg(Arg::new("version")
            .short('V')
            .long("version")
            .help("display the version number and exit")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("exclude")
            .short('e')
            .long("exclude")
            .action(ArgAction::Append)
            .value_name("COND")
            .help("skip specified records/fields")
        )
        .arg(Arg::new("include")
            .short('i')
            .long("include")
            .action(ArgAction::Append)
            .value_name("COND")
            .help("skip all but specified records/fields")
        )
        .arg(Arg::new("keep")
            .short('k')
            .long("keep")
            .help("keep (don't delete) input files")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("use_stdout")
            .short('c')
            .long("stdout")
            .conflicts_with("keep")
            .help("write on standard output, keep original files unchanged")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("newline")
            .short('n')
            .long("newline")
            .value_name("NL")
            .default_value(DEFAULT_NEWLINE)
            .value_parser(PossibleValuesParser::new([
                "unix",
                "dos",
            ]))
            .requires("disassemble")
            .help("newline style")
        )
        .dont_collapse_args_in_usage(true)
        .get_matches()
    ;
    if *args.get_one("version").unwrap() {
        println!(env!("CARGO_PKG_VERSION"));
        exit(0);
    }
    let files = args.get_many::<OsString>("FILE").map_or_else(Vec::new, |v| v.map(|v| if v == HYPHEN {
        None
    } else {
        Some(PathBuf::from(v))
    }).collect());
    let fit = *args.get_one("fit").unwrap();
    let keep = if *args.get_one("use_stdout").unwrap() {
        None
    } else {
        Some(*args.get_one("keep").unwrap())
    };
    let disassemble = if *args.get_one("disassemble").unwrap() {
        Some(match args.get_one::<String>("newline").unwrap().as_ref() {
            "dos" => "\r\n",
            "unix" => "\n",
            _ => unreachable!()
        })
    } else {
        None
    };
    let verbose = *args.get_one("verbose").unwrap();
    let code_page = match args.get_one::<String>("code_page").unwrap().as_ref() {
        "en" => CodePage::English,
        "ru" => CodePage::Russian,
        _ => unreachable!()
    };
    let (exclude_records, exclude_fields) = parse_conds(&args, "exclude");
    let (include_records, include_fields) = parse_conds(&args, "include");
    (Options {
        fit, keep, disassemble, verbose, code_page,
        exclude_records, exclude_fields, include_records, include_fields
    },
        files
    )
}
*/

fn main() -> ExitCode {
    let app = current_exe().ok()
        .and_then(|x| x.file_stem().map(|x| x.to_os_string()))
        .and_then(|x| x.into_string().ok())
        .map(|x| Box::leak(x.into_boxed_str()) as &str)
        .unwrap_or("potions_balance");
    let mut app = Command::new(app)
        .version(env!("CARGO_PKG_VERSION"))
        .disable_colored_help(true)
        .after_help("Report bugs to <internalmike@gmail.com> (in English or Russian).")
        .about("Helps to tune potions attributes.")
        .disable_help_subcommand(true)
        .disable_help_flag(true)
        .arg(Arg::new("help")
            .short('h')
            .long("help")
            .help("display this help and exit")
            .action(ArgAction::Help)
        )
        .disable_version_flag(true)
        .arg(Arg::new("version")
            .short('V')
            .long("version")
            .help("display the version number and exit")
            .action(ArgAction::SetTrue)
        )
        .subcommand(Command::new("scan")
            .about("scan enabled plugins and build base .esp file with all potions without modifications")
            .before_help("Scan <CONFIG FILE> for enabled plugins and build base .esp file with all potions without modifications")
            .help_template("Usage: {usage}\n\n{before-help}{options}")
            .arg(Arg::new("help")
                .short('h')
                .long("help")
                .help("display this help and exit")
                .action(ArgAction::Help)
            )
            .arg(Arg::new("CONFIG FILE")
                .required(true)
                .action(ArgAction::Set)
                .value_parser(value_parser!(OsString))
            )
            .arg(Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT.esp")
                .value_parser(value_parser!(OsString))
                .required(true)
                .help("output plugin file")
            )
            .arg(Arg::new("code_page")
                .short('p')
                .long("code-page")
                .value_name("LANG")
                .value_parser(PossibleValuesParser::new([
                    "en",
                    "ru",
                ]))
                .required(true)
                .help("text code page")
            )
        )
        .dont_collapse_args_in_usage(true)
    ;
    let args = app.clone().get_matches();
    if *args.get_one("version").unwrap() {
        println!(env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Err(err) = match args.subcommand() {
        Some(("scan", scan)) => command_scan(scan),
        Some((c, _)) => panic!("unknown command '{}'", c),
        None => {
            let _ = app.print_help();
            return ExitCode::from(2);
        },
    } {
        eprintln!("{}", err);
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

const MW_INI: &[u8] = "Morrowind.ini".as_bytes();
const MW_CFG: &[u8] = "openmw.cfg".as_bytes();

fn command_scan(args: &ArgMatches) -> Result<(), String> {
    let code_page = match args.get_one::<String>("code_page").unwrap().as_ref() {
        "en" => CodePage::English,
        "ru" => CodePage::Russian,
        _ => unreachable!()
    };
    let cfg = Path::new(args.get_one::<OsString>("CONFIG FILE").unwrap());
    let cfg = match unsafe { transmute(cfg.file_name()) } {
        Some(MW_INI) => parse_ini(cfg),
        Some(MW_CFG) => parse_cfg(cfg),
        _ => return Err("Unknown config file, supported files are 'Morrowind.ini', and 'openmw.cfg'.".into()),
    }?;
    let mut potions = HashMap::new();
    let mut max_time = None;
    for file_name in cfg.file_names {
        let file = cfg.data_folders.iter().rev()
            .map(|x| x.join(&file_name))
            .find(|x| fs::metadata(x).ok().map_or(false, |x| x.is_file()))
            .ok_or_else(|| format!("'{}' not found", file_name.to_string_lossy()))?
        ;
        if collect_potions(&file, &mut potions, code_page)? {
            let metadata = fs::metadata(file).map_err(|x| x.to_string())?;
            let time = FileTime::from_last_modification_time(&metadata);
            if max_time.map_or(true, |max_time| time > max_time) {
                max_time = Some(time)
            }
        }
    }
    let Some(max_time) = max_time else { return Err("potions not found".into()); };
    let max_time = max_time.unix_seconds();
    if i64::MAX - max_time < 120 { return Err("file time limit exceeded".into()); }
    let output_time = FileTime::from_unix_time(max_time + 120, 0);
    let mut records = Vec::new();
    records.push(Record {
        tag: TES3,
        flags: RecordFlags::empty(),
        fields: vec![
            (HEDR, Field::FileMetadata(FileMetadata {
                version: 1067869798,
                file_type: FileType::ESP,
                author: "potions_balance".to_string(),
                description: vec!["Potions balance.".into()],
                records: 0
            }))
        ]
    });
    records.extend(potions.into_values());
    let records_count = (records.len() - 1) as u32;
    if let Field::FileMetadata(f) = &mut records[0].fields[0].1 {
        f.records = records_count;
    } else {
        panic!()
    }
    let output = Path::new(args.get_one::<OsString>("output").unwrap());
    {
        let mut output = BufWriter::new(File::create(&output).map_err(|e| e.to_string())?);
        code::serialize_into(&records, &mut output, code_page, true).map_err(|e| e.to_string())?;
    }
    set_file_mtime(&output, output_time).map_err(|e| e.to_string())?;
    Ok(())
}

struct Config {
    data_folders: Vec<PathBuf>,
    file_names: Vec<OsString>,
}

fn parse_cfg(mw_cfg: &Path) -> Result<Config, String> {
    let mut ini = Vec::new();
    File::open(mw_cfg).and_then(|mut x| x.read_to_end(&mut ini)).map_err(|x| x.to_string())?;
    let ini = String::from_utf8(ini).map_err(|x| x.to_string())?;
    let ini = Ini::load_from_str(&ini).map_err(|x| x.to_string())?;
    let mut config = Config {
        data_folders: Vec::new(),
        file_names: Vec::new(),
    };
    for (key, value) in ini.general_section().iter() {
        match key {
            "data" => config.data_folders.push(PathBuf::from_str(value).unwrap()),
            "content" => config.file_names.push(OsString::from_str(value).unwrap()),
            _ => { },
        }
    }
    Ok(config)
}

fn parse_ini(mw_ini: &Path) -> Result<Config, String> {
    let data_folder = mw_ini.with_file_name("Data Files");
    let mut ini = Vec::new();
    File::open(mw_ini).and_then(|mut x| x.read_to_end(&mut ini)).map_err(|x| x.to_string())?;
    let ini = WINDOWS_1251.decode(&ini, DecoderTrap::Strict).map_err(|x| x.to_string())?;
    let ini = Ini::load_from_str(&ini).map_err(|x| x.to_string())?;
    let game_files_section = ini.section(Some("Game Files")).ok_or("The [Game Files] section is missing.")?;
    let mut game_files = Vec::with_capacity(game_files_section.len());
    for (_, name) in game_files_section.iter() {
        let path = data_folder.join(name);
        let metadata = fs::metadata(path.as_path()).map_err(|x| x.to_string())?;
        let time = FileTime::from_last_modification_time(&metadata);
        game_files.push((name, path, time));
    }
    game_files.sort_by_key(|x| x.2);
    game_files.sort_by_key(|x| x.1.extension().and_then(|e| e.to_str()).map(|e| e.to_uppercase()));
    Ok(Config {
        data_folders: vec![data_folder],
        file_names: game_files.iter().map(|x| OsString::from(x.0)).collect()
    })
}

fn collect_potions(path: &Path, potions: &mut HashMap<String, Record>, code_page: CodePage) -> Result<bool, String> {
    let mut file = File::open(path).map_err(|x| x.to_string())?;
    let mut records = Records::new(code_page, RecordReadMode::Lenient, 0, &mut file);
    let file_header = records.next().ok_or_else(|| format!("{}: invalid file", path.display()))?;
    let file_header = file_header.map_err(|_| format!("{}: invalid file", path.display()))?;
    let (_, file_header) = file_header.fields.first().ok_or_else(|| format!("{}: invalid file", path.display()))?;
    if let Field::FileMetadata(file_header) = file_header {
        if file_header.author == "potions_balance" { return Ok(false); }
    } else {
        return Err(format!("{}: invalid file", path.display()));
    }
    let mut has_potions = false;
    for record in records {
        let record = match record {
            Err(error) => match error.source() {
                Right(error) => return Err(format!("{}: {}", path.display(), error)),
                Left(error) => if error.record_tag() == ALCH {
                    return Err(format!("{}: {}", path.display(), error));
                } else {
                    continue;
                }
            },
            Ok(record) => record
        };
        if record.tag != ALCH { continue; }
        let id = if let Field::StringZ(ref id) = record.fields.iter().find(|(tag, _)| *tag == NAME)
            .ok_or_else(|| format!("{}: missing ID field in Alchemy record", path.display()))?.1 {
            id.string.to_uppercase()
        } else {
            panic!()
        };
        potions.insert(id, record);
        has_potions = true;
    }
    Ok(has_potions)
}
