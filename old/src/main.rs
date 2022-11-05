#![windows_subsystem = "windows"]
#![deny(warnings)]
#![allow(clippy::type_complexity)]
#![allow(clippy::or_fun_call)]

use winapi_gui::*;
use std::iter::once;
use std::str::FromStr;
use winapi::um::winuser::*;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use ini::Ini;
use std::fs::{self, File};
use filetime::{FileTime, set_file_mtime};
use std::io::{self, Read, BufWriter};
use encoding::all::WINDOWS_1251;
use encoding::Encoding;
use encoding::DecoderTrap;
use esl::{ALCH, Record, ENAM, NAME, Field, ALDT, FileMetadata, RecordFlags, FileType, TES3, HEDR, EffectIndex};
use esl::read::{Records, RecordReadMode};
use esl::code::{self, CodePage};
use either::{Right, Left};
use std::collections::{HashMap};
use winapi::shared::minwindef::{WPARAM};
use winreg::RegKey;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY};
use std::mem::transmute;
use dyn_fmt::AsStrFormatExt;

fn main() {
    let main_dialog_proc = &mut MainWindowProc { edit_original_value: None };
    if let Err(e) = dialog_box(None, 1, main_dialog_proc) {
        message_box(None, format!("{}.", e), load_string(3).unwrap(), MB_ICONERROR | MB_OK);
    }
}

static STANDARD: &[u16] = &[
    5, 15, 35, 80, 175,
    8, 15, 30, 45, 60,
    5, 8, 10, 15, 20,
    8, 15, 30, 45, 60,
    5, 8, 10, 15, 20,
    1, 2, 10, 20, 40,
    5, 15, 10, 15, 10,
    200, 1, 1
];

static RECOMMEND: &[u16] = &[
    20, 40, 80, 160, 320,
    20, 40, 80, 160, 320,
    10, 25, 45, 70, 100,
    20, 40, 80, 160, 320,
    10, 25, 45, 70, 100,
    5, 10, 17, 25, 40,
    5, 80, 45, 80, 45,
    125, 4, 5
];

fn find_morrowind() -> io::Result<Option<PathBuf>> {
    let programs = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#, KEY_READ | KEY_WOW64_32KEY)?;
    programs.enum_keys().filter_map(|x| x.ok()).find_map(|x| mfr_path(&programs, &x))
        .or_else(morrowind_path)
        .transpose()
}

fn morrowind_path() -> Option<io::Result<PathBuf>> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(r#"SOFTWARE\Bethesda Softworks\Morrowind"#, KEY_READ | KEY_WOW64_32KEY).ok()?;
    if key.get_raw_value("Installed Path").is_ok() {
        Some(get_folder(&key, "Installed Path"))
    } else {
        None
    }
}

fn mfr_path(programs: &RegKey, program: &str) -> Option<io::Result<PathBuf>> {
    let program = programs.open_subkey_with_flags(program, KEY_READ | KEY_WOW64_32KEY).ok()?;
    let id: String = program.get_value("HelpLink").ok()?;
    if &id != "http://www.fullrest.ru/forum/forum/300-morrowind-fullrest-repack-i-drugie-proekty-ot-ela/"
        && &id != "http://www.fullrest.ru/forum/topic/36164-morrowind-fullrest-repack/" {
        return None;
    }
    Some(get_folder(&program, "InstallLocation"))
}

fn get_folder(key: &RegKey, name: &str) -> io::Result<PathBuf> {
    let folder: OsString = key.get_value(name)?;
    let mut folder: Vec<u8> = unsafe { transmute(folder) };
    let trim_at = folder.iter().position(|&x| x == 0).unwrap_or_else(|| folder.len());
    folder.truncate(trim_at);
    let folder: OsString = unsafe { transmute(folder) };
    let mut folder = PathBuf::from(folder);
    folder.push("1");
    folder.pop();
    Ok(folder)
}

struct MainWindowProc {
    edit_original_value: Option<OsString>,
}

impl WindowProc for MainWindowProc {
    type DialogResult = io::Result<()>;

    fn wm_close(&mut self, window: Window<Self::DialogResult>, _wm: &mut Wm) {
        window.end_dialog(Ok(()))
    }

    fn wm_init_dialog(&mut self, window: Window<Self::DialogResult>, _wm: &mut WmInitDialog) -> io::Result<()> {
        let morrowind_path = find_morrowind().unwrap_or_else(|error| {
            message_box(None, format!("{}. {}.", load_string(5).unwrap(), error), load_string(4).unwrap(), MB_ICONINFORMATION | MB_OK);
            None
        });
        if let Some(morrowind_path) = morrowind_path {
            window.set_dialog_item_text(132, morrowind_path.as_os_str())?;
        }
        window.set_dialog_item_limit_text(132, 255);
        window.set_dialog_item_limit_text(134, 255);
        window.set_dialog_item_text_str(134, "PotionsBalance")?;
        for (i, _) in STANDARD.iter().enumerate() {
            window.set_dialog_item_limit_text(150 + i as u16, 3);
        }
        window.post_wm_command(129, 0)?;
        Ok(())
    }

    fn wm_command(&mut self, window: Window<Self::DialogResult>, command_id: u16, notification_code: u16, _wm: &mut Wm) -> io::Result<()> {
        match notification_code {
            EN_SETFOCUS => {
                if command_id != 132 && command_id != 134 {
                    debug_assert!(self.edit_original_value.is_none());
                    self.edit_original_value = Some(window.get_dialog_item_text(command_id, 4));
                }
            },
            EN_KILLFOCUS => {
                if command_id != 132 && command_id != 134 {
                    let edit_original_value = self.edit_original_value.take().unwrap();
                    let edit_value = window.get_dialog_item_text(command_id, 4);
                    let edit_value = edit_value.to_str().unwrap();
                    if edit_value.is_empty() || command_id != 185 && command_id != 186 && u16::from_str(edit_value).unwrap() == 0 {
                        window.set_dialog_item_text(command_id, &edit_original_value)?;
                    }
                }
            },
            _ => match command_id {
                129 => {
                    for (i, v) in STANDARD.iter().enumerate() {
                        window.set_dialog_item_text_str(150 + i as u16, &v.to_string())?;
                    }
                },
                130 => {
                    for (i, v) in RECOMMEND.iter().enumerate() {
                        window.set_dialog_item_text_str(150 + i as u16, &v.to_string())?;
                    }
                },
                133 => {
                    if let Some(file) = get_open_file_name(Some(&window), Some("Morrowind.ini"), once(("Morrowind.ini", "Morrowind.ini"))) {
                        window.set_dialog_item_text(132, &file.parent().unwrap().as_os_str())?;
                    }
                },
                127 => {
                    let mw_path = window.get_dialog_item_text(132, 256);
                    let esp_name = window.get_dialog_item_text(134, 256);
                    if mw_path.is_empty() {
                        message_box(Some(&window), load_string(6).unwrap(), load_string(3).unwrap(), MB_ICONWARNING | MB_OK);
                        window.get_dialog_item(132).unwrap().as_ref().set_focus()?;
                    } else if esp_name.is_empty() {
                        message_box(Some(&window), load_string(7).unwrap(), load_string(3).unwrap(), MB_ICONWARNING | MB_OK);
                        window.get_dialog_item(134).unwrap().as_ref().set_focus()?;
                    } else {
                        let mw_path = PathBuf::from(mw_path);
                        let values = (0..STANDARD.len()).map(|i| u16::from_str(window.get_dialog_item_text(150 + i as u16, 4).to_str().unwrap()).unwrap()).collect::<Vec<_>>();
                        let mut generating = GeneratingWindowProc { mw_path: &mw_path, esp_name: &esp_name, values: &values };
                        dialog_box(Some(&window), 2, &mut generating)?;
                    }
                },
                _ => { }
            }
        }
        Ok(())
    }
}

struct GeneratingWindowProc<'a, 'b, 'c> {
    mw_path: &'a Path,
    esp_name: &'b OsString,
    values: &'c [u16]
}

impl<'a, 'b, 'c> WindowProc for GeneratingWindowProc<'a, 'b, 'c> {
    type DialogResult = io::Result<()>;
    
    fn wm_init_dialog(&mut self, window: Window<Self::DialogResult>,_wm: &mut WmInitDialog) -> io::Result<()> {
        window.post_wm_timer(1)
    }

    fn wm_timer(&mut self, window: Window<Self::DialogResult>, id: WPARAM) -> io::Result<()> {
        if id == 1 {
            window.post_wm_timer(2)?;
        } else if id == 2 {
            if let Err(e) = generate_plugin(&self.mw_path, &self.esp_name, &self.values) {
                message_box(Some(&window), e, load_string(3).unwrap(), MB_ICONERROR | MB_OK);
            } else {
                message_box(Some(&window), load_string(8).unwrap().format(&[self.esp_name.to_string_lossy()]), load_string(9).unwrap(), MB_ICONINFORMATION | MB_OK);
            }
            window.end_dialog(Ok(()));
        }
        Ok(())
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum PotionLevel {
    Bargain = 0,
    Cheap = 1,
    Standard = 2,
    Quality = 3,
    Exclusive = 4
}

#[allow(clippy::question_mark)]
fn potion_level_kind(id: &str, record: &Record) -> Option<(PotionLevel,  String)> {
    let mut effects = record.fields.iter().filter(|(tag, _)| *tag == ENAM);
    if effects.next().is_none() { return None; }
    if effects.next().is_some() { return None; }
    if id.starts_with("P_") {
        if id.ends_with("_B") {
            Some((PotionLevel::Bargain, id[2 .. id.len() - 2].replace(' ', "_")))
        } else if id.ends_with("_C") {
            Some((PotionLevel::Cheap, id[2 .. id.len() - 2].replace(' ', "_")))
        } else if id.ends_with("_S") {
            Some((PotionLevel::Standard, id[2 .. id.len() - 2].replace(' ', "_")))
        } else if id.ends_with("_Q") {
            Some((PotionLevel::Quality, id[2 .. id.len() - 2].replace(' ', "_")))
        } else if id.ends_with("_E") {
            Some((PotionLevel::Exclusive, id[2 .. id.len() - 2].replace(' ', "_")))
        } else {
            None
        }
    } else {
        None
    }
}

fn potion_value(record: &Record) -> Result<u32, String> {
    let data = record.fields.iter().find(|(tag, _)| *tag == ALDT).ok_or_else(|| load_string(10).unwrap())?;
    if let Field::Potion(data) = &data.1 {
        Ok(data.value)
    } else {
        panic!()
    }
}

fn potion_effect(record: &Record) -> Result<EffectIndex, String> {
    let data = record.fields.iter().find(|(tag, _)| *tag == ENAM).unwrap();
    if let Field::Effect(data) = &data.1 {
        data.index.right().ok_or_else(|| load_string(10).unwrap())
    } else {
        panic!()
    }
}

fn generate_plugin(mw_path: &Path, esp_name: &OsString, values: &[u16]) -> Result<(), String> {
    let (potions, file_time) = collect_potions(mw_path)?;
    let (potions_by_kind, standard_only_potions, _) = classify_potions(potions)?;
    let level_values = find_level_values(&potions_by_kind)?;
    let mut records = Vec::new();
    records.push(Record {
        tag: TES3,
        flags: RecordFlags::empty(),
        fields: vec![
            (HEDR, Field::FileMetadata(FileMetadata {
                version: 1067869798,
                file_type: FileType::ESP,
                author: "PotionsBalance.exe".to_string(),
                description: vec![load_string(11).unwrap(), format!("{:?}", values)],
                records: 0
            }))
        ]
    });
    for (_, mut potion) in standard_only_potions.into_iter() {
        if set_potion(&mut potion, &level_values, values, None)? {
            records.push(potion);
        }
    }
    for (_, potions) in potions_by_kind.into_iter() {
        for (level, (_, mut potion)) in potions.to_vec().into_iter().enumerate().filter_map(|(i, x)| x.map(|u| (i, u))) {
            if set_potion(&mut potion, &level_values, values, Some(level as u8))? {
                records.push(potion);
            }
        }
    }
    let records_count = (records.len() - 1) as u32;
    if let Field::FileMetadata(f) = &mut records[0].fields[0].1 {
        f.records = records_count;
    } else {
        panic!()
    }
    let esp_path = mw_path.join("Data Files").join(esp_name).with_extension("esp");
    {
        let mut esp = BufWriter::new(File::create(&esp_path).map_err(|e| e.to_string())?);
        code::serialize_into(&records, &mut esp, CodePage::Russian, true).map_err(|e| e.to_string())?;
    }
    set_file_mtime(&esp_path, FileTime::from_unix_time(file_time.unix_seconds() + 120, 0)).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum EffectAttributes {
    None,
    Duration,
    Magnitude,
    DurationAndMagnitude,
    CommonDurationAndMagnitude,
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
        EffectIndex::RestoreHealth => EffectAttributes::CommonDurationAndMagnitude,
        EffectIndex::RestoreSpellPoints => EffectAttributes::CommonDurationAndMagnitude,
        EffectIndex::RestoreFatigue => EffectAttributes::CommonDurationAndMagnitude,
        _ => EffectAttributes::DurationAndMagnitude
    }
}

fn set_potion(record: &mut Record, level_values: &[u32], values: &[u16], level: Option<u8>) -> Result<bool, String> {
    let mut changed = false;
    changed |= set_potion_value(record, level_values, values)?;
    changed |= set_potion_weight(record, values)?;
    let effect = potion_effect(record)?;
    match effect_attributes(effect) {
        EffectAttributes::None => { },
        EffectAttributes::Duration => {
            if let Some(level) = level {
                set_potion_duration(record, values[15 + level as usize]);
            } else {
                set_potion_duration(record, values[33]);
            }
        },
        EffectAttributes::Magnitude => {
            if let Some(level) = level {
                set_potion_magnitude(record, values[20 + level as usize]);
            } else {
                set_potion_magnitude(record, values[34]);
            }
        },
        EffectAttributes::DurationAndMagnitude => {
            if let Some(level) = level {
                set_potion_duration(record, values[5 + level as usize]);
                set_potion_magnitude(record, values[10 + level as usize]);
            } else {
                set_potion_duration(record, values[31]);
                set_potion_magnitude(record, values[32]);
            }
        },
        EffectAttributes::CommonDurationAndMagnitude => {
            if let Some(level) = level {
                set_potion_duration(record, values[30]);
                set_potion_magnitude(record, values[25 + level as usize]);
            } else {
                set_potion_duration(record, values[31]);
                set_potion_magnitude(record, values[32]);
            }
        }
    }
    Ok(changed)
}

fn set_potion_duration(record: &mut Record, duration: u16) {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ENAM).unwrap();
    if let Field::Effect(data) = &mut data.1 {
        data.duration = duration as i32;
    } else {
        panic!()
    }
}

fn set_potion_magnitude(record: &mut Record, magnitude: u16) {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ENAM).unwrap();
    if let Field::Effect(data) = &mut data.1 {
        data.magnitude_min = magnitude as i32;
        data.magnitude_max = data.magnitude_min;
    } else {
        panic!()
    }
}

fn set_potion_value(record: &mut Record, level_values: &[u32], values: &[u16]) -> Result<bool, String> {
    let mut values = values[.. 5].to_vec();
    values.sort_unstable();
    let old_value = potion_value(record)?;
    let new_value = match level_values.binary_search(&old_value) {
        Ok(i) => values[i] as u32,
        Err(i) => if i == 0 {
            (old_value as f64 * values[0] as f64 / level_values[0] as f64).round() as u32
        } else if i == 5 {
            (values[4] as f64 + (values[4] - values[3]) as f64 * (1.0 + (old_value - level_values[4]) as f64 / (level_values[4] - level_values[3]) as f64)).round() as u32
        } else {
            (values[i - 1] as f64 + (values[i] - values[i - 1]) as f64 * (old_value - level_values[i - 1]) as f64 / (level_values[i] - level_values[i - 1]) as f64).round() as u32 
        }
    };
    if new_value == old_value { return Ok(false); }
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ALDT).unwrap();
    if let Field::Potion(data) = &mut data.1 {
        data.value = new_value;
    } else {
        panic!()
    }
    Ok(true)
}

#[allow(clippy::float_cmp)]
fn set_potion_weight(record: &mut Record, values: &[u16]) -> Result<bool, String> {
    let data = record.fields.iter_mut().find(|(tag, _)| *tag == ALDT).unwrap();
    if let Field::Potion(data) = &mut data.1 {
        let new_weight = ((data.weight as f64).min(values[35] as f64 * 0.01) * values[36] as f64 / values[37] as f64) as f32;
        if new_weight == data.weight { return Ok(false) };
        data.weight = new_weight;
    } else {
        panic!()
    }
    Ok(true)
}

fn find_level_values(potions: &HashMap<String, [Option<(String, Record)>; 5]>) -> Result<[u32; 5], String> {
    let mut level_values = [0; 5];
    for (level, level_value) in level_values.iter_mut().enumerate() {
        let mut values = Vec::new();
        for potion in potions.iter().filter_map(|x| x.1[level].as_ref()) {
            values.push(potion_value(&potion.1)?);
        }
        values.sort_unstable();
        if values.is_empty() {
            return Err(load_string(12).unwrap());
        }
        *level_value = values[values.len() / 2];
    }
    level_values.sort_unstable();
    Ok(level_values)
}

fn classify_potions(potions: HashMap<String, Record>)
    -> Result<(HashMap<String, [Option<(String, Record)>; 5]>, Vec<(String, Record)>, Vec<(String, Record)>), String> {

    let mut potions_by_normalized_id = HashMap::new();
    for (id, record) in potions.into_iter() {
        let normalized_id = id.replace(' ', "_");
        if let Some((existing_id, _)) = potions_by_normalized_id.insert(normalized_id, (id.clone(), record)) {
            return Err(load_string(13).unwrap().format(&[id, existing_id]));
        }
    }
    let mut potions_by_kind = HashMap::new();
    let mut other_potions = Vec::new();
    for (normalized_id, (id, record)) in potions_by_normalized_id.into_iter() {
        if let Some((level, kind)) = potion_level_kind(&normalized_id, &record) {
            let entry = potions_by_kind.entry(kind).or_insert_with(|| [None, None, None, None, None]);
            entry[level as usize] = Some((id, record));
        } else {
            other_potions.push((id, record));
        }
    }
    let mut standard_only_potions_ids = Vec::new();
    let mut standard_only_potions = Vec::new();
    for (normalized_id, levels) in potions_by_kind.iter_mut() {
        if levels[0].is_none() && levels[1].is_none() && levels[3].is_none() && levels[4].is_none() {
            standard_only_potions_ids.push(normalized_id.clone());
            standard_only_potions.push(levels[2].take().unwrap());
        }
    }
    for normalized_id in standard_only_potions_ids {
        potions_by_kind.remove(&normalized_id);
    }
    Ok((potions_by_kind, standard_only_potions, other_potions))
}

fn collect_potions(mw_path: &Path) -> Result<(HashMap<String, Record>, FileTime), String> {
    let mut ini = Vec::new();
    File::open(mw_path.join("Morrowind.ini").as_path()).and_then(|mut x| x.read_to_end(&mut ini)).map_err(|x| x.to_string())?;
    let ini = WINDOWS_1251.decode(&ini, DecoderTrap::Strict).map_err(|x| x.to_string())?;
    let ini = Ini::load_from_str(&ini).map_err(|x| x.to_string())?;
    let game_files_section = ini.section(Some("Game Files")).ok_or(load_string(14).unwrap())?;
    let mut game_files = Vec::with_capacity(game_files_section.len());
    for (_, name) in game_files_section.iter() {
        let path = mw_path.join("Data Files").join(name);
        let metadata = fs::metadata(path.as_path()).map_err(|x| x.to_string())?;
        let time = FileTime::from_last_modification_time(&metadata);
        game_files.push((name, path, time));
    }
    game_files.sort_by_key(|x| x.2);
    game_files.sort_by_key(|x| x.1.extension().and_then(|e| e.to_str()).map(|e| e.to_uppercase()));
    let mut file_time = None;
    let mut potions = HashMap::new();
    for (game_file_name, game_file_path, game_file_time) in game_files.into_iter() {
        let mut has_potions = false;
        let mut game_file = File::open(game_file_path).map_err(|x| x.to_string())?;
        let mut records = Records::new(CodePage::Russian, RecordReadMode::Lenient, 0, &mut game_file);
        let file_header = records.next().ok_or_else(|| load_string(15).unwrap())?.map_err(|e| e.to_string())?;
        let (_, file_header) = file_header.fields.first().ok_or_else(|| load_string(15).unwrap())?;
        if let Field::FileMetadata(file_header) = file_header {
            if file_header.author == "PotionsBalance.exe" { continue; }
        } else {
            return Err(load_string(15).unwrap());
        }
        for record in records {
            let record = match record {
                Err(error) => match error.source() {
                    Right(error) => return Err(format!("{}: {}", game_file_name, error)),
                    Left(error) => if error.record_tag() == ALCH {
                        return Err(format!("{}: {}", game_file_name, error));
                    } else {
                        continue;
                    }
                },
                Ok(record) => record
            };
            if record.tag != ALCH { continue; }
            let id = if let Field::StringZ(ref id) = record.fields.iter().find(|(tag, _)| *tag == NAME)
                .ok_or(load_string(16).unwrap().format(&[game_file_name]))?.1 {
                id.string.to_uppercase()
            } else {
                panic!()
            };
            potions.insert(id, record);
            has_potions = true;
        }
        if has_potions {
            file_time = Some(game_file_time);
        }
    }
    if let Some(file_time) = file_time {
        Ok((potions, file_time))
    } else {
        Err(load_string(17).unwrap())
    }
}
