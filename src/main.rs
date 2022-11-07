#![feature(result_flattening)]

#![deny(warnings)]

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use clap::builder::PossibleValuesParser;
use csv::StringRecord;
use either::{Left, Right};
use encoding::{DecoderTrap, Encoding};
use encoding::all::WINDOWS_1251;
use esl::{ALCH, ALDT, ENAM, Field, FileMetadata, FileType, HEDR, NAME, Record, RecordFlags, TES3};
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

fn main() -> ExitCode {
    let app = current_exe().ok()
        .and_then(|x| x.file_stem().map(|x| x.to_os_string()))
        .and_then(|x| x.into_string().ok())
        .map(|x| Box::leak(x.into_boxed_str()) as &str)
        .unwrap_or("potions-balance");
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
                .help("the game language")
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
        .subcommand(Command::new("apply")
            .about("Apply .csv file with potions attributes to base .esp file")
            .before_help("Apply <SOURCE.csv> with potions attributes to <TARGET.esp> file")
            .help_template("Usage: {usage}\n\n{before-help}{options}")
            .arg(Arg::new("help")
                .short('h')
                .long("help")
                .help("display this help and exit")
                .action(ArgAction::Help)
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
                .help("the game language")
            )
            .arg(Arg::new("source")
                .short('s')
                .long("source")
                .value_name("SOURCE.csv")
                .value_parser(value_parser!(OsString))
                .required(true)
                .help("source .csv file")
            )
            .arg(Arg::new("TARGET.esp")
                .required(true)
                .action(ArgAction::Set)
                .value_parser(value_parser!(OsString))
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
        Some(("apply", apply)) => command_apply(apply),
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

fn command_apply(args: &ArgMatches) -> Result<(), String> {
    let source = Path::new(args.get_one::<OsString>("source").unwrap());
    let balance = {
        let mut source = csv::Reader::from_path(source).map_err(|e| e.to_string())?;
        let source = source.records().map(|x| x.map_err(|e| e.to_string()));
        Balance::from_csv(source).map_err(|e| e.unwrap_or_else(|| "Invalid .csv file.".into()))?
    };
    let code_page = match args.get_one::<String>("code_page").unwrap().as_ref() {
        "en" => CodePage::English,
        "ru" => CodePage::Russian,
        _ => unreachable!()
    };
    let target = Path::new(args.get_one::<OsString>("TARGET.esp").unwrap());
    let metadata = fs::metadata(target).map_err(|x| x.to_string())?;
    let time = FileTime::from_last_modification_time(&metadata);
    let mut potions = HashMap::new();
    collect_potions(target, &mut potions, code_page, false)?;
    for potion in potions.values_mut() {
        patch_potion(potion, &balance)?;
    }
    write_potions(target, potions, time, code_page)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum Quality {
    Bargain = 0,
    Cheap = 1,
    Standard = 2,
    Quality = 3,
    Exclusive = 4
}

fn potion_quality_and_effect(
    record: &Record,
) -> Result<Option<(Option<Quality>, EffectIndex)>, String> {
    let Field::Potion(data) = &record.fields.iter().find(|(tag, _)| *tag == ALDT).unwrap().1 else { panic!() };
    if data.auto_calculate_value { return Ok(None); }
    let id = if let Field::StringZ(ref id) = record.fields.iter().find(|(tag, _)| *tag == NAME).unwrap().1 {
        id.string.to_uppercase()
    } else {
        panic!()
    };
    let mut effects = record.fields.iter().filter(|(tag, _)| *tag == ENAM);
    let Some(effect) = effects.next() else { return Ok(None); };
    if effects.next().is_some() { return Ok(None); }
    let effect = if let Field::Effect(effect) = &effect.1 {
        effect.index.right().ok_or_else(|| format!("Invalid potion '{}'.", id))?
    } else {
        panic!()
    };
    if effect_kind(effect) == EffectKind::Damage {
        return Ok(None);
    }
    if effect_attributes(effect).is_none() {
        return Ok(Some((None, effect)));
    }
    let quality = if id.ends_with("_B") || id.ends_with("_B_CHG") {
        Some(Quality::Bargain)
    } else if id.ends_with("_C") || id.ends_with("_C_CHG") {
        Some(Quality::Cheap)
    } else if id.ends_with("_S") || id.ends_with("_S_CHG") {
        Some(Quality::Standard)
    } else if id.ends_with("_Q") || id.ends_with("_Q_CHG") {
        Some(Quality::Quality)
    } else if id.ends_with("_E") || id.ends_with("_E_CHG") {
        Some(Quality::Exclusive)
    } else {
        None
    };
    Ok(Some((quality, effect)))
}

fn potion_value(quality: Option<Quality>, effect: EffectIndex, balance: &Balance) -> Option<u32> {
    Some(match (quality, effect) {
        (None, EffectIndex::Mark) => balance.without_quality_value.mark,
        (None, EffectIndex::Recall) => balance.without_quality_value.teleport,
        (None, EffectIndex::DivineIntervention) => balance.without_quality_value.teleport,
        (None, EffectIndex::AlmsiviIntervention) => balance.without_quality_value.teleport,
        (None, EffectIndex::CurePoison) => balance.without_quality_value.cure_poison_or_paralyzation,
        (None, EffectIndex::CureParalyzation) => balance.without_quality_value.cure_poison_or_paralyzation,
        (None, EffectIndex::CureCommonDisease) => balance.without_quality_value.cure_common_disease,
        (None, EffectIndex::CureBlightDisease) => balance.without_quality_value.cure_blight_disease,
        (None, EffectIndex::Vampirism) => balance.without_quality_value.vampirism,
        (None, _) => return None,
        (Some(Quality::Bargain), _) => balance.with_quality_value.bargain,
        (Some(Quality::Cheap), _) => balance.with_quality_value.cheap,
        (Some(Quality::Standard), _) => balance.with_quality_value.standard,
        (Some(Quality::Quality), _) => balance.with_quality_value.quality,
        (Some(Quality::Exclusive), _) => balance.with_quality_value.exclusive,
    })
}

fn potion_weight(quality: Option<Quality>, effect: EffectIndex, balance: &Balance) -> Option<f32> {
    Some(match (quality, effect) {
        (None, EffectIndex::Mark) => balance.without_quality_weight.mark,
        (None, EffectIndex::Recall) => balance.without_quality_weight.teleport,
        (None, EffectIndex::DivineIntervention) => balance.without_quality_weight.teleport,
        (None, EffectIndex::AlmsiviIntervention) => balance.without_quality_weight.teleport,
        (None, EffectIndex::CurePoison) => balance.without_quality_weight.cure_poison_or_paralyzation,
        (None, EffectIndex::CureParalyzation) => balance.without_quality_weight.cure_poison_or_paralyzation,
        (None, EffectIndex::CureCommonDisease) => balance.without_quality_weight.cure_common_disease,
        (None, EffectIndex::CureBlightDisease) => balance.without_quality_weight.cure_blight_disease,
        (None, EffectIndex::Vampirism) => balance.without_quality_weight.vampirism,
        (None, _) => return None,
        (Some(Quality::Bargain), _) => balance.with_quality_weight.bargain,
        (Some(Quality::Cheap), _) => balance.with_quality_weight.cheap,
        (Some(Quality::Standard), _) => balance.with_quality_weight.standard,
        (Some(Quality::Quality), _) => balance.with_quality_weight.quality,
        (Some(Quality::Exclusive), _) => balance.with_quality_weight.exclusive,
    })
}

fn potion_duration(quality: Quality, effect: EffectIndex, balance: &Balance) -> Option<i32> {
    let effect_attributes = effect_attributes(effect).unwrap();
    let restore = effect_kind(effect) == EffectKind::Restore;
    Some(match (quality, effect_attributes, restore) {
        (Quality::Bargain, EffectAttributes::Duration, _) =>
            balance.duration_only.bargain,
        (Quality::Bargain, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.bargain.0,
        (Quality::Bargain, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.bargain.0,
        (Quality::Cheap, EffectAttributes::Duration, _) =>
            balance.duration_only.cheap,
        (Quality::Cheap, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.cheap.0,
        (Quality::Cheap, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.cheap.0,
        (Quality::Standard, EffectAttributes::Duration, _) =>
            balance.duration_only.standard,
        (Quality::Standard, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.standard.0,
        (Quality::Standard, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.standard.0,
        (Quality::Quality, EffectAttributes::Duration, _) =>
            balance.duration_only.quality,
        (Quality::Quality, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.quality.0,
        (Quality::Quality, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.quality.0,
        (Quality::Exclusive, EffectAttributes::Duration, _) =>
            balance.duration_only.exclusive,
        (Quality::Exclusive, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.exclusive.0,
        (Quality::Exclusive, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.exclusive.0,
        _ => return None
    })
}

fn potion_magnitude(quality: Quality, effect: EffectIndex, balance: &Balance) -> Option<i32> {
    let effect_attributes = effect_attributes(effect).unwrap();
    let restore = effect_kind(effect) == EffectKind::Restore;
    Some(match (quality, effect_attributes, restore) {
        (Quality::Bargain, EffectAttributes::Magnitude, _) =>
            balance.magnitude_only.bargain,
        (Quality::Bargain, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.bargain.1,
        (Quality::Bargain, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.bargain.1,
        (Quality::Cheap, EffectAttributes::Magnitude, _) =>
            balance.magnitude_only.cheap,
        (Quality::Cheap, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.cheap.1,
        (Quality::Cheap, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.cheap.1,
        (Quality::Standard, EffectAttributes::Magnitude, _) =>
            balance.magnitude_only.standard,
        (Quality::Standard, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.standard.1,
        (Quality::Standard, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.standard.1,
        (Quality::Quality, EffectAttributes::Magnitude, _) =>
            balance.magnitude_only.quality,
        (Quality::Quality, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.quality.1,
        (Quality::Quality, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.quality.1,
        (Quality::Exclusive, EffectAttributes::Magnitude, _) =>
            balance.magnitude_only.exclusive,
        (Quality::Exclusive, EffectAttributes::DurationAndMagnitude, true) =>
            balance.restore_duration_and_magnitude.exclusive.1,
        (Quality::Exclusive, EffectAttributes::DurationAndMagnitude, false) =>
            balance.others_duration_and_magnitude.exclusive.1,
        _ => return None
    })
}

fn patch_potion(record: &mut Record, balance: &Balance) -> Result<(), String> {
    let Some((quality, effect)) = potion_quality_and_effect(record)? else {
        return Ok(());
    };
    if let Some(value) = potion_value(quality, effect, balance) {
        set_potion_value(record, value);
    }
    if let Some(weight) = potion_weight(quality, effect, balance) {
        set_potion_weight(record, weight);
    }
    let Some(quality) = quality else { return Ok(()); };
    if let Some(duration) = potion_duration(quality, effect, balance) {
        set_potion_duration(record, duration);
    }
    if let Some(magnitude) = potion_magnitude(quality, effect, balance) {
        set_potion_magnitude(record, magnitude);
    }
    Ok(())
}

fn set_potion_magnitude(record: &mut Record, value: i32) {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ENAM).unwrap();
    let Field::Effect(data) = &mut data.1 else { panic!() };
    data.magnitude_min = value;
    data.magnitude_max = value;
}

fn set_potion_duration(record: &mut Record, value: i32) {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ENAM).unwrap();
    let Field::Effect(data) = &mut data.1 else { panic!() };
    data.duration = value;
}

fn set_potion_value(record: &mut Record, value: u32) {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ALDT).unwrap();
    let Field::Potion(data) = &mut data.1 else { panic!() };
    data.value = value;
}

fn set_potion_weight(record: &mut Record, value: f32) {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ALDT).unwrap();
    let Field::Potion(data) = &mut data.1 else { panic!() };
    data.weight = value;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum EffectKind {
    Damage,
    Restore,
    Other,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum EffectAttributes {
    Duration,
    Magnitude,
    DurationAndMagnitude,
}

fn effect_kind(effect: EffectIndex) -> EffectKind {
    match effect {
        EffectIndex::FireDamage => EffectKind::Damage,
        EffectIndex::FrostDamage => EffectKind::Damage,
        EffectIndex::ShockDamage => EffectKind::Damage,
        EffectIndex::RestoreHealth => EffectKind::Restore,
        EffectIndex::RestoreSpellPoints => EffectKind::Restore,
        EffectIndex::RestoreFatigue => EffectKind::Restore,
        EffectIndex::DamageHealth => EffectKind::Restore,
        EffectIndex::DamageMagicka => EffectKind::Restore,
        EffectIndex::DamageFatigue => EffectKind::Restore,
        _ => EffectKind::Other,
    }
}

fn effect_attributes(effect: EffectIndex) -> Option<EffectAttributes> {
    match effect {
        EffectIndex::WaterBreathing => Some(EffectAttributes::Duration),
        EffectIndex::WaterWalking => Some(EffectAttributes::Duration),
        EffectIndex::Invisibility => Some(EffectAttributes::Duration),
        EffectIndex::Paralyze => Some(EffectAttributes::Duration),
        EffectIndex::Silence => Some(EffectAttributes::Duration),
        EffectIndex::Dispel => Some(EffectAttributes::Magnitude),
        EffectIndex::Mark => None,
        EffectIndex::Recall => None,
        EffectIndex::DivineIntervention => None,
        EffectIndex::AlmsiviIntervention => None,
        EffectIndex::CureCommonDisease => None,
        EffectIndex::CureBlightDisease => None,
        EffectIndex::CureCorprusDisease => None,
        EffectIndex::CurePoison => None,
        EffectIndex::CureParalyzation => None,
        EffectIndex::RestoreAttribute => Some(EffectAttributes::Magnitude),
        EffectIndex::RestoreSkill => Some(EffectAttributes::Magnitude),
        EffectIndex::SummonScamp => Some(EffectAttributes::Duration),
        EffectIndex::SummonClannfear => Some(EffectAttributes::Duration),
        EffectIndex::SummonDaedroth => Some(EffectAttributes::Duration),
        EffectIndex::SummonDremora => Some(EffectAttributes::Duration),
        EffectIndex::SummonAncestralGhost => Some(EffectAttributes::Duration),
        EffectIndex::SummonSkeletalMinion => Some(EffectAttributes::Duration),
        EffectIndex::SummonLeastBonewalker => Some(EffectAttributes::Duration),
        EffectIndex::SummonGreaterBonewalker => Some(EffectAttributes::Duration),
        EffectIndex::SummonBonelord => Some(EffectAttributes::Duration),
        EffectIndex::SummonWingedTwilight => Some(EffectAttributes::Duration),
        EffectIndex::SummonHunger => Some(EffectAttributes::Duration),
        EffectIndex::SummonGoldensaint => Some(EffectAttributes::Duration),
        EffectIndex::SummonFlameAtronach => Some(EffectAttributes::Duration),
        EffectIndex::SummonFrostAtronach => Some(EffectAttributes::Duration),
        EffectIndex::SummonStormAtronach => Some(EffectAttributes::Duration),
        EffectIndex::BoundDagger => Some(EffectAttributes::Duration),
        EffectIndex::BoundLongsword => Some(EffectAttributes::Duration),
        EffectIndex::BoundMace => Some(EffectAttributes::Duration),
        EffectIndex::BoundBattleAxe => Some(EffectAttributes::Duration),
        EffectIndex::BoundSpear => Some(EffectAttributes::Duration),
        EffectIndex::BoundLongbow => Some(EffectAttributes::Duration),
        EffectIndex::BoundCuirass => Some(EffectAttributes::Duration),
        EffectIndex::BoundHelm => Some(EffectAttributes::Duration),
        EffectIndex::BoundBoots => Some(EffectAttributes::Duration),
        EffectIndex::BoundShield => Some(EffectAttributes::Duration),
        EffectIndex::BoundGloves => Some(EffectAttributes::Duration),
        EffectIndex::Corpus => Some(EffectAttributes::Duration),
        EffectIndex::Vampirism => None,
        EffectIndex::SummonCenturionSphere => Some(EffectAttributes::Duration),
        EffectIndex::SummonFabricant => Some(EffectAttributes::Duration),
        EffectIndex::SummonCreature01 => Some(EffectAttributes::Duration),
        EffectIndex::SummonCreature02 => Some(EffectAttributes::Duration),
        EffectIndex::SummonCreature03 => Some(EffectAttributes::Duration),
        EffectIndex::SummonCreature04 => Some(EffectAttributes::Duration),
        EffectIndex::SummonCreature05 => Some(EffectAttributes::Duration),
        EffectIndex::StuntedMagicka => Some(EffectAttributes::Duration),
        _ => Some(EffectAttributes::DurationAndMagnitude)
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
    fn from_csv(mut csv: impl Iterator<Item=Result<StringRecord, String>>) -> Result<Self, Option<String>> {
        let mut balance = Balance {
            without_quality_value: WithoutQuality {
                mark: 0,
                teleport: 0,
                cure_common_disease: 0,
                cure_blight_disease: 0,
                cure_poison_or_paralyzation: 0,
                vampirism: 0,
            },
            with_quality_value: WithQuality {
                bargain: 0,
                cheap: 0,
                standard: 0,
                quality: 0,
                exclusive: 0,
            },
            without_quality_weight: WithoutQuality {
                mark: 0.0,
                teleport: 0.0,
                cure_common_disease: 0.0,
                cure_blight_disease: 0.0,
                cure_poison_or_paralyzation: 0.0,
                vampirism: 0.0,
            },
            with_quality_weight: WithQuality {
                bargain: 0.0,
                cheap: 0.0,
                standard: 0.0,
                quality: 0.0,
                exclusive: 0.0,
            },
            duration_only: WithQuality {
                bargain: 0,
                cheap: 0,
                standard: 0,
                quality: 0,
                exclusive: 0,
            },
            magnitude_only: WithQuality {
                bargain: 0,
                cheap: 0,
                standard: 0,
                quality: 0,
                exclusive: 0,
            },
            restore_duration_and_magnitude: WithQuality {
                bargain: (0, 0),
                cheap: (0, 0),
                standard: (0, 0),
                quality: (0, 0),
                exclusive: (0, 0),
            },
            others_duration_and_magnitude: WithQuality {
                bargain: (0, 0),
                cheap: (0, 0),
                standard: (0, 0),
                quality: (0, 0),
                exclusive: (0, 0),
            },
        };
        let row_bargain = csv.next().ok_or(None)?.map_err(Some)?;
        balance.with_quality_value.bargain = row_bargain.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.with_quality_weight.bargain = row_bargain.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        balance.duration_only.bargain = row_bargain.get(3).ok_or(None)?.parse().map_err(|_| None)?;
        balance.magnitude_only.bargain = row_bargain.get(4).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.bargain.0 = row_bargain.get(5).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.bargain.1 = row_bargain.get(6).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.bargain.0 = row_bargain.get(7).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.bargain.1 = row_bargain.get(8).ok_or(None)?.parse().map_err(|_| None)?;
        let row_cheap = csv.next().ok_or(None)?.map_err(Some)?;
        balance.with_quality_value.cheap = row_cheap.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.with_quality_weight.cheap = row_cheap.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        balance.duration_only.cheap = row_cheap.get(3).ok_or(None)?.parse().map_err(|_| None)?;
        balance.magnitude_only.cheap = row_cheap.get(4).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.cheap.0 = row_cheap.get(5).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.cheap.1 = row_cheap.get(6).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.cheap.0 = row_cheap.get(7).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.cheap.1 = row_cheap.get(8).ok_or(None)?.parse().map_err(|_| None)?;
        let row_standard = csv.next().ok_or(None)?.map_err(Some)?;
        balance.with_quality_value.standard = row_standard.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.with_quality_weight.standard = row_standard.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        balance.duration_only.standard = row_standard.get(3).ok_or(None)?.parse().map_err(|_| None)?;
        balance.magnitude_only.standard = row_standard.get(4).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.standard.0 = row_standard.get(5).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.standard.1 = row_standard.get(6).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.standard.0 = row_standard.get(7).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.standard.1 = row_standard.get(8).ok_or(None)?.parse().map_err(|_| None)?;
        let row_quality = csv.next().ok_or(None)?.map_err(Some)?;
        balance.with_quality_value.quality = row_quality.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.with_quality_weight.quality = row_quality.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        balance.duration_only.quality = row_quality.get(3).ok_or(None)?.parse().map_err(|_| None)?;
        balance.magnitude_only.quality = row_quality.get(4).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.quality.0 = row_quality.get(5).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.quality.1 = row_quality.get(6).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.quality.0 = row_quality.get(7).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.quality.1 = row_quality.get(8).ok_or(None)?.parse().map_err(|_| None)?;
        let row_exclusive = csv.next().ok_or(None)?.map_err(Some)?;
        balance.with_quality_value.exclusive = row_exclusive.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.with_quality_weight.exclusive = row_exclusive.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        balance.duration_only.exclusive = row_exclusive.get(3).ok_or(None)?.parse().map_err(|_| None)?;
        balance.magnitude_only.exclusive = row_exclusive.get(4).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.exclusive.0 = row_exclusive.get(5).ok_or(None)?.parse().map_err(|_| None)?;
        balance.restore_duration_and_magnitude.exclusive.1 = row_exclusive.get(6).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.exclusive.0 = row_exclusive.get(7).ok_or(None)?.parse().map_err(|_| None)?;
        balance.others_duration_and_magnitude.exclusive.1 = row_exclusive.get(8).ok_or(None)?.parse().map_err(|_| None)?;
        csv.next().ok_or(None)?.map_err(Some)?;
        csv.next().ok_or(None)?.map_err(Some)?;
        let row_mark = csv.next().ok_or(None)?.map_err(Some)?;
        balance.without_quality_value.mark = row_mark.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.without_quality_weight.mark = row_mark.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        let row_teleport = csv.next().ok_or(None)?.map_err(Some)?;
        balance.without_quality_value.teleport = row_teleport.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.without_quality_weight.teleport = row_teleport.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        let row_cure_p_or_p = csv.next().ok_or(None)?.map_err(Some)?;
        balance.without_quality_value.cure_poison_or_paralyzation =
            row_cure_p_or_p.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.without_quality_weight.cure_poison_or_paralyzation =
            row_cure_p_or_p.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        let row_cure_c_disease = csv.next().ok_or(None)?.map_err(Some)?;
        balance.without_quality_value.cure_common_disease = row_cure_c_disease.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.without_quality_weight.cure_common_disease = row_cure_c_disease.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        let row_cure_b_disease = csv.next().ok_or(None)?.map_err(Some)?;
        balance.without_quality_value.cure_blight_disease = row_cure_b_disease.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.without_quality_weight.cure_blight_disease = row_cure_b_disease.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        let row_vampirism = csv.next().ok_or(None)?.map_err(Some)?;
        balance.without_quality_value.vampirism = row_vampirism.get(1).ok_or(None)?.parse().map_err(|_| None)?;
        balance.without_quality_weight.vampirism = row_vampirism.get(2).ok_or(None)?.parse().map_err(|_| None)?;
        Ok(balance)
    }

    fn to_csv(&self) -> Vec<StringRecord> {
        let mut rows = Vec::new();
        let mut row_headers = StringRecord::new();
        row_headers.push_field("");
        row_headers.push_field("Value");
        row_headers.push_field("Weight");
        row_headers.push_field("Duration Only");
        row_headers.push_field("Magnitude Only");
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
        let mut row_empty = StringRecord::new();
        for _ in 0 .. 9 {
            row_empty.push_field("");
        }
        rows.push(row_empty);
        let mut row_headers = StringRecord::new();
        row_headers.push_field("");
        row_headers.push_field("Value");
        row_headers.push_field("Weight");
        for _ in 0 .. 6 {
            row_headers.push_field("");
        }
        rows.push(row_headers);
        let mut row_mark = StringRecord::new();
        row_mark.push_field("Mark");
        row_mark.push_field(&self.without_quality_value.mark.to_string());
        row_mark.push_field(&self.without_quality_weight.mark.to_string());
        for _ in 0 .. 6 {
            row_mark.push_field("");
        }
        rows.push(row_mark);
        let mut row_teleport = StringRecord::new();
        row_teleport.push_field("Teleport");
        row_teleport.push_field(&self.without_quality_value.teleport.to_string());
        row_teleport.push_field(&self.without_quality_weight.teleport.to_string());
        for _ in 0 .. 6 {
            row_teleport.push_field("");
        }
        rows.push(row_teleport);
        let mut row_cure_poison_or_paralyzation = StringRecord::new();
        row_cure_poison_or_paralyzation.push_field("Cure Poison / Paralyzation");
        row_cure_poison_or_paralyzation.push_field(&self.without_quality_value.cure_poison_or_paralyzation.to_string());
        row_cure_poison_or_paralyzation.push_field(&self.without_quality_weight.cure_poison_or_paralyzation.to_string());
        for _ in 0 .. 6 {
            row_cure_poison_or_paralyzation.push_field("");
        }
        rows.push(row_cure_poison_or_paralyzation);
        let mut row_cure_common_disease = StringRecord::new();
        row_cure_common_disease.push_field("Cure Common Disease");
        row_cure_common_disease.push_field(&self.without_quality_value.cure_common_disease.to_string());
        row_cure_common_disease.push_field(&self.without_quality_weight.cure_common_disease.to_string());
        for _ in 0 .. 6 {
            row_cure_common_disease.push_field("");
        }
        rows.push(row_cure_common_disease);
        let mut row_cure_blight_disease = StringRecord::new();
        row_cure_blight_disease.push_field("Cure Blight Disease");
        row_cure_blight_disease.push_field(&self.without_quality_value.cure_blight_disease.to_string());
        row_cure_blight_disease.push_field(&self.without_quality_weight.cure_blight_disease.to_string());
        for _ in 0 .. 6 {
            row_cure_blight_disease.push_field("");
        }
        rows.push(row_cure_blight_disease);
        let mut row_vampirism = StringRecord::new();
        row_vampirism.push_field("Vampirism");
        row_vampirism.push_field(&self.without_quality_value.vampirism.to_string());
        row_vampirism.push_field(&self.without_quality_weight.vampirism.to_string());
        for _ in 0 .. 6 {
            row_vampirism.push_field("");
        }
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
    {
        let mut output = csv::Writer::from_path(output).map_err(|e| e.to_string())?;
        for row in balance.to_csv() {
            output.write_record(&row).map_err(|e| e.to_string())?;
        }
    }
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
        if collect_potions(&file, &mut potions, code_page, true)? {
            let metadata = fs::metadata(file).map_err(|x| x.to_string())?;
            let time = FileTime::from_last_modification_time(&metadata);
            if max_time.map_or(true, |max_time| time > max_time) {
                max_time = Some(time)
            }
        }
    }
    let Some(max_time) = max_time else { return Err("Potions not found.".into()); };
    let max_time = max_time.unix_seconds();
    if i64::MAX - max_time < 120 { return Err("File is too new: time limit exceeded.".into()); }
    let output_time = FileTime::from_unix_time(max_time + 120, 0);
    let output = Path::new(args.get_one::<OsString>("output").unwrap());
    write_potions(output, potions, output_time, code_page)
}

fn write_potions(
    output: &Path,
    potions: HashMap<String, Record>,
    time: FileTime,
    code_page: CodePage
) -> Result<(), String> {
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
    {
        let mut output = BufWriter::new(File::create(output).map_err(|e| e.to_string())?);
        code::serialize_into(&records, &mut output, code_page, true).map_err(|e| e.to_string())?;
    }
    set_file_mtime(output, time).map_err(|e| e.to_string())?;
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

fn collect_potions(
    path: &Path,
    potions: &mut HashMap<String, Record>,
    code_page: CodePage,
    skip_balance_plugin: bool,
) -> Result<bool, String> {
    let mut file = File::open(path).map_err(|x| x.to_string())?;
    let mut records = Records::new(code_page, RecordReadMode::Lenient, 0, &mut file);
    let file_header = records.next().ok_or_else(|| format!("'{}': invalid file.", path.display()))?;
    let file_header = file_header.map_err(|_| format!("'{}': invalid file.", path.display()))?;
    let (_, file_header) = file_header.fields.first().ok_or_else(|| format!("'{}': invalid file.", path.display()))?;
    if let Field::FileMetadata(file_header) = file_header {
        if skip_balance_plugin && file_header.author == "potions_balance" { return Ok(false); }
    } else {
        return Err(format!("'{}': invalid file.", path.display()));
    }
    let mut has_potions = false;
    for record in records {
        let record = match record {
            Err(error) => match error.source() {
                Right(error) => return Err(format!("'{}': {}.", path.display(), error)),
                Left(error) => if error.record_tag() == ALCH {
                    return Err(format!("'{}': {}.", path.display(), error));
                } else {
                    continue;
                }
            },
            Ok(record) => record
        };
        if record.tag != ALCH { continue; }
        let id = if let Field::StringZ(ref id) = record.fields.iter().find(|(tag, _)| *tag == NAME)
            .ok_or_else(|| format!("'{}': missing NAME field in ALCH record.", path.display()))?.1 {
            id.string.to_uppercase()
        } else {
            panic!()
        };
        potions.insert(id, record);
        has_potions = true;
    }
    Ok(has_potions)
}
