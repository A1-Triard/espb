use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use clap::builder::PossibleValuesParser;
use csv::StringRecord;
use either::{Left, Right};
use encoding::{DecoderTrap, Encoding};
use encoding::all::WINDOWS_1251;
use esl::{ALCH, Field, FileMetadata, FileType, HEDR, NAME, Record, RecordFlags, TES3};
use esl::EffectIndex;
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
        .about("Morrowind potions attributes balancing tool.")
        .help_template("Usage: {usage}\n\n{about}\n\n{subcommands}\n\n{options}{after-help}")
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
            .about("Scan game config and build .esp file with all potions")
            .before_help("\
                Scan <CONFIG FILE> for enabled plugins and build <OUTPUT.esp> file \
                with all potions (without additional modifications)\
            ")
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
        .subcommand(Command::new("init")
            .about("Create .csv file with potions attributes info")
            .before_help("Create <OUTPUT.csv> with potions attributes info")
            .help_template("Usage: {usage}\n\n{before-help}{options}")
            .arg(Arg::new("help")
                .short('h')
                .long("help")
                .help("display this help and exit")
                .action(ArgAction::Help)
            )
            .arg(Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT.csv")
                .value_parser(value_parser!(OsString))
                .required(true)
                .help("output .csv file")
            )
            .arg(Arg::new("type")
                .short('t')
                .long("type")
                .value_name("TYPE")
                .value_parser(PossibleValuesParser::new([
                    "original",
                    "recommended",
                ]))
                .required(true)
                .help("selects one of predefined balances")
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
        Some(("init", init)) => command_init(init),
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

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum EffectKind {
    Restore,
    Other,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum EffectAttributes {
    None,
    Duration,
    Magnitude,
    DurationAndMagnitude,
}

fn effect_kind(effect: EffectIndex) -> EffectKind {
    match effect {
        EffectIndex::RestoreHealth => EffectKind::Restore,
        EffectIndex::RestoreSpellPoints => EffectKind::Restore,
        EffectIndex::RestoreFatigue => EffectKind::Restore,
        _ => EffectKind::Other,
    }
}

fn effect_attributes(effect: EffectIndex) -> EffectAttributes {
    match effect {
        EffectIndex::WaterBreathing => EffectAttributes::Duration,
        EffectIndex::WaterWalking => EffectAttributes::Duration,
        EffectIndex::Invisibility => EffectAttributes::Duration,
        EffectIndex::Paralyze => EffectAttributes::Duration,
        EffectIndex::Silence => EffectAttributes::Duration,
        EffectIndex::Dispel => EffectAttributes::Magnitude,
        EffectIndex::Mark => EffectAttributes::None,
        EffectIndex::Recall => EffectAttributes::None,
        EffectIndex::DivineIntervention => EffectAttributes::None,
        EffectIndex::AlmsiviIntervention => EffectAttributes::None,
        EffectIndex::CureCommonDisease => EffectAttributes::None,
        EffectIndex::CureBlightDisease => EffectAttributes::None,
        EffectIndex::CureCorprusDisease => EffectAttributes::None,
        EffectIndex::CurePoison => EffectAttributes::None,
        EffectIndex::CureParalyzation => EffectAttributes::None,
        EffectIndex::RestoreAttribute => EffectAttributes::Magnitude,
        EffectIndex::RestoreSkill => EffectAttributes::Magnitude,
        EffectIndex::SummonScamp => EffectAttributes::Duration,
        EffectIndex::SummonClannfear => EffectAttributes::Duration,
        EffectIndex::SummonDaedroth => EffectAttributes::Duration,
        EffectIndex::SummonDremora => EffectAttributes::Duration,
        EffectIndex::SummonAncestralGhost => EffectAttributes::Duration,
        EffectIndex::SummonSkeletalMinion => EffectAttributes::Duration,
        EffectIndex::SummonLeastBonewalker => EffectAttributes::Duration,
        EffectIndex::SummonGreaterBonewalker => EffectAttributes::Duration,
        EffectIndex::SummonBonelord => EffectAttributes::Duration,
        EffectIndex::SummonWingedTwilight => EffectAttributes::Duration,
        EffectIndex::SummonHunger => EffectAttributes::Duration,
        EffectIndex::SummonGoldensaint => EffectAttributes::Duration,
        EffectIndex::SummonFlameAtronach => EffectAttributes::Duration,
        EffectIndex::SummonFrostAtronach => EffectAttributes::Duration,
        EffectIndex::SummonStormAtronach => EffectAttributes::Duration,
        EffectIndex::BoundDagger => EffectAttributes::Duration,
        EffectIndex::BoundLongsword => EffectAttributes::Duration,
        EffectIndex::BoundMace => EffectAttributes::Duration,
        EffectIndex::BoundBattleAxe => EffectAttributes::Duration,
        EffectIndex::BoundSpear => EffectAttributes::Duration,
        EffectIndex::BoundLongbow => EffectAttributes::Duration,
        EffectIndex::BoundCuirass => EffectAttributes::Duration,
        EffectIndex::BoundHelm => EffectAttributes::Duration,
        EffectIndex::BoundBoots => EffectAttributes::Duration,
        EffectIndex::BoundShield => EffectAttributes::Duration,
        EffectIndex::BoundGloves => EffectAttributes::Duration,
        EffectIndex::Corpus => EffectAttributes::Duration,
        EffectIndex::Vampirism => EffectAttributes::None,
        EffectIndex::SummonCenturionSphere => EffectAttributes::Duration,
        EffectIndex::SummonFabricant => EffectAttributes::Duration,
        EffectIndex::SummonCreature01 => EffectAttributes::Duration,
        EffectIndex::SummonCreature02 => EffectAttributes::Duration,
        EffectIndex::SummonCreature03 => EffectAttributes::Duration,
        EffectIndex::SummonCreature04 => EffectAttributes::Duration,
        EffectIndex::SummonCreature05 => EffectAttributes::Duration,
        EffectIndex::StuntedMagicka => EffectAttributes::Duration,
        _ => EffectAttributes::DurationAndMagnitude
    }
}

struct WithQuality<T> {
    bargain: T,
    cheap: T,
    standard: T,
    quality: T,
    exclusive: T,
}

struct WithoutQuality<T> {
    mark: T,
    teleport: T,
    cure_common_disease: T,
    cure_blight_disease: T,
    cure_poison_or_paralyzation: T,
    vampirism: T,
}

struct Balance {
    without_quality_value: WithoutQuality<u32>,
    with_quality_value: WithQuality<u32>,
    without_quality_weight: WithoutQuality<f32>,
    with_quality_weight: WithQuality<f32>,
    duration_only: WithQuality<i32>,
    magnitude_only: WithQuality<i32>,
    restore_duration_and_magnitude: WithQuality<(i32, i32)>,
    others_duration_and_magnitude: WithQuality<(i32, i32)>,
}

impl Balance {
    fn to_csv(&self) -> Vec<StringRecord> {
        let mut rows = Vec::new();
        let mut row_headers = StringRecord::new();
        row_headers.push_field("");
        row_headers.push_field("Value");
        row_headers.push_field("Weight");
        row_headers.push_field("Duration Only");
        row_headers.push_field("Magnitide Only");
        row_headers.push_field("Restore Duration");
        row_headers.push_field("Restore Magnitude");
        row_headers.push_field("Others Duration");
        row_headers.push_field("Others Magnitude");
        rows.push(row_headers);
        let mut row_bargain = StringRecord::new();
        row_bargain.push_field("Bargain");
        row_bargain.push_field(&self.with_quality_value.bargain.to_string());
        row_bargain.push_field(&self.with_quality_weight.bargain.to_string());
        row_bargain.push_field(&self.duration_only.bargain.to_string());
        row_bargain.push_field(&self.magnitude_only.bargain.to_string());
        row_bargain.push_field(&self.restore_duration_and_magnitude.bargain.0.to_string());
        row_bargain.push_field(&self.restore_duration_and_magnitude.bargain.1.to_string());
        row_bargain.push_field(&self.others_duration_and_magnitude.bargain.0.to_string());
        row_bargain.push_field(&self.others_duration_and_magnitude.bargain.1.to_string());
        rows.push(row_bargain);
        let mut row_cheap = StringRecord::new();
        row_cheap.push_field("Cheap");
        row_cheap.push_field(&self.with_quality_value.cheap.to_string());
        row_cheap.push_field(&self.with_quality_weight.cheap.to_string());
        row_cheap.push_field(&self.duration_only.cheap.to_string());
        row_cheap.push_field(&self.magnitude_only.cheap.to_string());
        row_cheap.push_field(&self.restore_duration_and_magnitude.cheap.0.to_string());
        row_cheap.push_field(&self.restore_duration_and_magnitude.cheap.1.to_string());
        row_cheap.push_field(&self.others_duration_and_magnitude.cheap.0.to_string());
        row_cheap.push_field(&self.others_duration_and_magnitude.cheap.1.to_string());
        rows.push(row_cheap);
        let mut row_standard = StringRecord::new();
        row_standard.push_field("Standard");
        row_standard.push_field(&self.with_quality_value.standard.to_string());
        row_standard.push_field(&self.with_quality_weight.standard.to_string());
        row_standard.push_field(&self.duration_only.standard.to_string());
        row_standard.push_field(&self.magnitude_only.standard.to_string());
        row_standard.push_field(&self.restore_duration_and_magnitude.standard.0.to_string());
        row_standard.push_field(&self.restore_duration_and_magnitude.standard.1.to_string());
        row_standard.push_field(&self.others_duration_and_magnitude.standard.0.to_string());
        row_standard.push_field(&self.others_duration_and_magnitude.standard.1.to_string());
        rows.push(row_standard);
        let mut row_quality = StringRecord::new();
        row_quality.push_field("Quality");
        row_quality.push_field(&self.with_quality_value.quality.to_string());
        row_quality.push_field(&self.with_quality_weight.quality.to_string());
        row_quality.push_field(&self.duration_only.quality.to_string());
        row_quality.push_field(&self.magnitude_only.quality.to_string());
        row_quality.push_field(&self.restore_duration_and_magnitude.quality.0.to_string());
        row_quality.push_field(&self.restore_duration_and_magnitude.quality.1.to_string());
        row_quality.push_field(&self.others_duration_and_magnitude.quality.0.to_string());
        row_quality.push_field(&self.others_duration_and_magnitude.quality.1.to_string());
        rows.push(row_quality);
        let mut row_exclusive = StringRecord::new();
        row_exclusive.push_field("Exclusive");
        row_exclusive.push_field(&self.with_quality_value.exclusive.to_string());
        row_exclusive.push_field(&self.with_quality_weight.exclusive.to_string());
        row_exclusive.push_field(&self.duration_only.exclusive.to_string());
        row_exclusive.push_field(&self.magnitude_only.exclusive.to_string());
        row_exclusive.push_field(&self.restore_duration_and_magnitude.exclusive.0.to_string());
        row_exclusive.push_field(&self.restore_duration_and_magnitude.exclusive.1.to_string());
        row_exclusive.push_field(&self.others_duration_and_magnitude.exclusive.0.to_string());
        row_exclusive.push_field(&self.others_duration_and_magnitude.exclusive.1.to_string());
        rows.push(row_exclusive);
        rows.push(StringRecord::new());
        let mut row_headers = StringRecord::new();
        row_headers.push_field("");
        row_headers.push_field("Value");
        row_headers.push_field("Weight");
        rows.push(row_headers);
        let mut row_mark = StringRecord::new();
        row_mark.push_field("Mark");
        row_mark.push_field(&self.without_quality_value.mark.to_string());
        row_mark.push_field(&self.without_quality_weight.mark.to_string());
        rows.push(row_mark);
        let mut row_teleport = StringRecord::new();
        row_teleport.push_field("Teleport");
        row_teleport.push_field(&self.without_quality_value.teleport.to_string());
        row_teleport.push_field(&self.without_quality_weight.teleport.to_string());
        rows.push(row_teleport);
        let mut row_cure_common_disease = StringRecord::new();
        row_cure_common_disease.push_field("Cure Common Disease");
        row_cure_common_disease.push_field(&self.without_quality_value.cure_common_disease.to_string());
        row_cure_common_disease.push_field(&self.without_quality_weight.cure_common_disease.to_string());
        rows.push(row_cure_common_disease);
        let mut row_cure_blight_disease = StringRecord::new();
        row_cure_blight_disease.push_field("Cure Blight Disease");
        row_cure_blight_disease.push_field(&self.without_quality_value.cure_blight_disease.to_string());
        row_cure_blight_disease.push_field(&self.without_quality_weight.cure_blight_disease.to_string());
        rows.push(row_cure_blight_disease);
        let mut row_cure_poison_or_paralyzation = StringRecord::new();
        row_cure_poison_or_paralyzation.push_field("Cure Poison/Paralyzation");
        row_cure_poison_or_paralyzation.push_field(&self.without_quality_value.cure_poison_or_paralyzation.to_string());
        row_cure_poison_or_paralyzation.push_field(&self.without_quality_weight.cure_poison_or_paralyzation.to_string());
        rows.push(row_cure_poison_or_paralyzation);
        let mut row_vampirism = StringRecord::new();
        row_vampirism.push_field("Vampirism");
        row_vampirism.push_field(&self.without_quality_value.vampirism.to_string());
        row_vampirism.push_field(&self.without_quality_weight.vampirism.to_string());
        rows.push(row_vampirism);
        rows
    }
}

static ORIGINAL: Balance = Balance {
    without_quality_value: WithoutQuality {
        mark: 35,
        teleport: 35,
        cure_common_disease: 20,
        cure_blight_disease: 30,
        cure_poison_or_paralyzation: 20,
        vampirism: 5000,
    },
    with_quality_value: WithQuality {
        bargain: 5,
        cheap: 15,
        standard: 35,
        quality: 80,
        exclusive: 175,
    },
    without_quality_weight: WithoutQuality {
        mark: 1.0,
        teleport: 1.0,
        cure_common_disease: 0.5,
        cure_blight_disease: 0.5,
        cure_poison_or_paralyzation: 0.5,
        vampirism: 1.5,
    },
    with_quality_weight: WithQuality {
        bargain: 1.5,
        cheap: 1.0,
        standard: 0.75,
        quality: 0.5,
        exclusive: 0.25,
    },
    duration_only: WithQuality {
        bargain: 8,
        cheap: 15,
        standard: 30,
        quality: 45,
        exclusive: 60,
    },
    magnitude_only: WithQuality {
        bargain: 5,
        cheap: 8,
        standard: 10,
        quality: 15,
        exclusive: 20,
    },
    restore_duration_and_magnitude: WithQuality {
        bargain: (5, 1),
        cheap: (5, 2),
        standard: (5, 10),
        quality: (5, 20),
        exclusive: (5, 40),
    },
    others_duration_and_magnitude: WithQuality {
        bargain: (8, 5),
        cheap: (15, 8),
        standard: (30, 10),
        quality: (45, 15),
        exclusive: (60, 20),
    },
};

static RECOMMENDED: Balance = Balance {
    without_quality_value: WithoutQuality {
        mark: 60,
        teleport: 120,
        cure_common_disease: 60,
        cure_blight_disease: 120,
        cure_poison_or_paralyzation: 60,
        vampirism: 5000,
    },
    with_quality_value: WithQuality {
        bargain: 20,
        cheap: 40,
        standard: 80,
        quality: 160,
        exclusive: 320,
    },
    without_quality_weight: WithoutQuality {
        mark: 0.8,
        teleport: 0.8,
        cure_common_disease: 0.4,
        cure_blight_disease: 0.4,
        cure_poison_or_paralyzation: 0.4,
        vampirism: 1.0,
    },
    with_quality_weight: WithQuality {
        bargain: 1.0,
        cheap: 0.8,
        standard: 0.6,
        quality: 0.4,
        exclusive: 0.2,
    },
    duration_only: WithQuality {
        bargain: 20,
        cheap: 40,
        standard: 80,
        quality: 160,
        exclusive: 320,
    },
    magnitude_only: WithQuality {
        bargain: 10,
        cheap: 25,
        standard: 45,
        quality: 70,
        exclusive: 100,
    },
    restore_duration_and_magnitude: WithQuality {
        bargain: (5, 5),
        cheap: (5, 10),
        standard: (5, 17),
        quality: (5, 25),
        exclusive: (5, 40),
    },
    others_duration_and_magnitude: WithQuality {
        bargain: (20, 10),
        cheap: (40, 25),
        standard: (80, 45),
        quality: (160, 70),
        exclusive: (320, 100),
    },
};

fn command_init(args: &ArgMatches) -> Result<(), String> {
    let balance = match args.get_one::<String>("type").unwrap().as_ref() {
        "original" => &ORIGINAL,
        "recommended" => &RECOMMENDED,
        _ => unreachable!()
    };
    let output = Path::new(args.get_one::<OsString>("output").unwrap());
    /*
    {
        let mut output = BufWriter::new(File::create(&output).map_err(|e| e.to_string())?);
        code::serialize_into(&records, &mut output, code_page, true).map_err(|e| e.to_string())?;
    }
    */
    Ok(())
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
