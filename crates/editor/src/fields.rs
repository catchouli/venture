//! The property panel: any content value, shown and edited through its
//! reflection.
//!
//! - Records show as a grid of fields, with readable names ("Armour class")
//!   and the field's doc comment as its tooltip.
//! - Lists have buttons to add, remove and reorder.
//! - Choices (enums) have a dropdown, which can switch to another kind.
//! - Three numbers from 0 to 1 are a colour, with a colour picker.
//! - Dice are written as text, such as "2d6+1".
//! - Text marked with [`Refers`] (an item's id, an icon's file) offers the
//!   names that exist, and shows a name that doesn't in red.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::reflect::enums::{DynamicEnum, DynamicVariant, Enum, VariantInfo};
use bevy::reflect::list::DynamicList;
use bevy::reflect::structs::DynamicStruct;
use bevy::reflect::tuple::DynamicTuple;
use bevy::reflect::{PartialReflect, ReflectMut, TypeInfo, TypeRegistry};
use bevy_egui::egui;
use venture_content::Refers;
use venture_rpg::Dice;

/// The names each kind of reference can be (see [`Refers`]).
#[derive(Resource, Default, Clone)]
pub struct Choices(pub HashMap<Refers, Vec<String>>);

/// What editing a value needs.
pub struct Fields<'a> {
    /// The type registry (for defaults when switching kinds or adding to lists).
    pub registry: &'a TypeRegistry,
    /// The names references can be.
    pub choices: &'a Choices,
}

impl Fields<'_> {
    /// Shows a value to edit. Returns whether it changed.
    pub fn edit(&self, ui: &mut egui::Ui, value: &mut dyn PartialReflect) -> bool {
        self.value(ui, value, None, "")
    }

    fn value(&self, ui: &mut egui::Ui, value: &mut dyn PartialReflect, refers: Option<Refers>, name: &str) -> bool {
        if let Some(changed) = self.special(ui, value, refers, name) {
            return changed;
        }
        match value.reflect_mut() {
            ReflectMut::Struct(record) => {
                let info = match record.get_represented_type_info() {
                    Some(TypeInfo::Struct(info)) => Some(info),
                    _ => None,
                };
                let mut changed = false;
                let id = ui.next_auto_id();
                egui::Grid::new(id).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    for index in 0..record.field_len() {
                        let field_name = record.name_at(index).unwrap_or_default().to_string();
                        let field_info = info.and_then(|info| info.field_at(index));
                        let label = ui.label(readable(&field_name));
                        if let Some(docs) = field_info.and_then(|field| field.docs()) {
                            label.on_hover_text(docs.trim());
                        }
                        let refers = field_info.and_then(|field| field.get_attribute::<Refers>()).copied();
                        if let Some(field) = record.field_at_mut(index) {
                            ui.push_id(index, |ui| {
                                ui.vertical(|ui| changed |= self.value(ui, field, refers, &field_name));
                            });
                        }
                        ui.end_row();
                    }
                });
                changed
            }
            ReflectMut::TupleStruct(tuple) => {
                let mut changed = false;
                ui.horizontal(|ui| {
                    for index in 0..tuple.field_len() {
                        if let Some(field) = tuple.field_mut(index) {
                            ui.push_id(index, |ui| changed |= self.value(ui, field, refers, name));
                        }
                    }
                });
                changed
            }
            ReflectMut::Tuple(tuple) => {
                let mut changed = false;
                ui.horizontal(|ui| {
                    for index in 0..tuple.field_len() {
                        if let Some(field) = tuple.field_mut(index) {
                            ui.push_id(index, |ui| changed |= self.value(ui, field, refers, name));
                        }
                    }
                });
                changed
            }
            ReflectMut::List(list) => self.list(ui, list, refers, name),
            ReflectMut::Map(map) => {
                let mut changed = false;
                let keys: Vec<Box<dyn PartialReflect>> = map.iter().map(|(key, _)| key.to_dynamic()).collect();
                for (index, key) in keys.iter().enumerate() {
                    let label = key.try_downcast_ref::<String>().cloned().unwrap_or_else(|| format!("{key:?}"));
                    let Some(value) = map.get_mut(key.as_ref()) else { continue };
                    egui::CollapsingHeader::new(&label).id_salt(index).show(ui, |ui| {
                        changed |= self.value(ui, value, refers, name);
                    });
                }
                changed
            }
            ReflectMut::Enum(choice) => self.choice(ui, choice, refers, name),
            ReflectMut::Opaque(value) => opaque(ui, value.as_partial_reflect_mut(), name),
            _ => {
                ui.weak(format!("{value:?}"));
                false
            }
        }
    }

    /// Values with a look of their own: colours, dice, references.
    fn special(
        &self,
        ui: &mut egui::Ui,
        value: &mut dyn PartialReflect,
        refers: Option<Refers>,
        name: &str,
    ) -> Option<bool> {
        // (Three numbers are a colour, unless they're a place or a turn.)
        if !matches!(name, "position" | "rotation" | "offset")
            && let Some(colour) = value.try_downcast_mut::<(f32, f32, f32)>()
        {
            return Some(colour_edit(ui, colour));
        }
        if let Some(dice) = value.try_downcast_mut::<Dice>() {
            return Some(dice_edit(ui, dice));
        }
        let refers = refers?;
        if let Some(text) = value.try_downcast_mut::<String>() {
            return Some(self.reference(ui, text, refers, false));
        }
        if let Some(maybe) = value.try_downcast_mut::<Option<String>>() {
            let mut text = maybe.clone().unwrap_or_default();
            let changed = self.reference(ui, &mut text, refers, true);
            if changed {
                *maybe = (!text.is_empty()).then_some(text);
            }
            return Some(changed);
        }
        None
    }

    /// A name from a list: a dropdown of the names that exist.
    fn reference(&self, ui: &mut egui::Ui, text: &mut String, refers: Refers, optional: bool) -> bool {
        let names = self.choices.0.get(&refers).cloned().unwrap_or_default();
        let missing = !text.is_empty() && !names.contains(text);
        let shown = if text.is_empty() { "(none)".to_string() } else { text.clone() };
        let shown = if missing { egui::RichText::new(shown).color(egui::Color32::LIGHT_RED) } else { shown.into() };
        let before = text.clone();
        egui::ComboBox::from_id_salt(ui.next_auto_id()).selected_text(shown).width(220.0).show_ui(ui, |ui| {
            if optional {
                ui.selectable_value(text, String::new(), "(none)");
            }
            for name in &names {
                ui.selectable_value(text, name.clone(), name);
            }
        });
        if missing {
            ui.colored_label(egui::Color32::LIGHT_RED, "There's nothing called that.");
        }
        *text != before
    }

    fn list(
        &self,
        ui: &mut egui::Ui,
        list: &mut dyn bevy::reflect::list::List,
        refers: Option<Refers>,
        name: &str,
    ) -> bool {
        let mut changed = false;
        let mut remove = None;
        let mut swap = None;
        for index in 0..list.len() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    if let Some(item) = list.get_mut(index) {
                        ui.vertical(|ui| changed |= self.value(ui, item, refers, name));
                    }
                    if ui.small_button("x").on_hover_text("Remove").clicked() {
                        remove = Some(index);
                    }
                    if index > 0 && ui.small_button("⏶").on_hover_text("Move up").clicked() {
                        swap = Some(index - 1);
                    }
                });
            });
        }
        if let Some(index) = remove {
            list.remove(index);
            changed = true;
        }
        if let Some(index) = swap {
            let item = list.remove(index + 1);
            list.insert(index, item);
            changed = true;
        }
        if ui.small_button("+ Add").clicked() {
            // A copy of the last one, or a blank one.
            let new = match list.len() {
                0 => match list.get_represented_type_info() {
                    Some(TypeInfo::List(info)) => info.item_info().and_then(|item| default_for(item, self.registry)),
                    _ => None,
                },
                len => list.get(len - 1).map(|last| last.to_dynamic()),
            };
            if let Some(new) = new {
                list.push(new);
                changed = true;
            }
        }
        changed
    }

    fn choice(&self, ui: &mut egui::Ui, choice: &mut dyn Enum, refers: Option<Refers>, name: &str) -> bool {
        let Some(TypeInfo::Enum(info)) = choice.get_represented_type_info() else {
            ui.weak(choice.variant_name());
            return false;
        };
        let mut changed = false;
        let current = choice.variant_name().to_string();
        let mut wanted = current.clone();
        if info.variant_len() > 1 {
            egui::ComboBox::from_id_salt(ui.next_auto_id()).selected_text(readable(&current)).show_ui(ui, |ui| {
                for variant in info.iter() {
                    ui.selectable_value(&mut wanted, variant.name().to_string(), readable(variant.name()));
                }
            });
        }
        if wanted != current
            && let Some(new) = info.variant(&wanted).and_then(|variant| default_variant(variant, self.registry))
        {
            choice.apply(&new);
            changed = true;
        }
        // What's inside the chosen kind.
        match choice.reflect_mut() {
            ReflectMut::Enum(choice) if choice.field_len() > 0 => {
                let variant = info.variant(choice.variant_name());
                for index in 0..choice.field_len() {
                    let field_name = choice.name_at(index).map(str::to_string);
                    if let Some(field_name) = &field_name {
                        ui.label(readable(field_name));
                    }
                    let docs = match variant {
                        Some(VariantInfo::Struct(variant)) => variant.field_at(index).and_then(|field| field.docs()),
                        _ => None,
                    };
                    let _ = docs;
                    if let Some(field) = choice.field_at_mut(index) {
                        ui.push_id(index, |ui| {
                            changed |= self.value(ui, field, refers, field_name.as_deref().unwrap_or(name));
                        });
                    }
                }
            }
            _ => {}
        }
        changed
    }
}

/// Plain values: text, numbers and ticks.
fn opaque(ui: &mut egui::Ui, value: &mut dyn PartialReflect, name: &str) -> bool {
    macro_rules! number {
        ($($kind:ty),*) => {$(
            if let Some(number) = value.try_downcast_mut::<$kind>() {
                return ui.add(egui::DragValue::new(number)).changed();
            }
        )*};
    }
    if let Some(text) = value.try_downcast_mut::<String>() {
        let long = matches!(name, "description" | "notes" | "journal");
        let edit =
            if long { egui::TextEdit::multiline(text).desired_rows(2) } else { egui::TextEdit::singleline(text) };
        return ui.add(edit.desired_width(360.0)).changed();
    }
    if let Some(tick) = value.try_downcast_mut::<bool>() {
        return ui.checkbox(tick, "").changed();
    }
    if let Some(number) = value.try_downcast_mut::<f32>() {
        // (Turns in whole degrees; places to the millimetre.)
        let (speed, decimals) = match name {
            "rotation" => (1.0, 1),
            "position" => (0.005, 3),
            _ => (0.05, 2),
        };
        return ui.add(egui::DragValue::new(number).speed(speed).max_decimals(decimals)).changed();
    }
    number!(f64, i8, i16, i32, i64, u8, u16, u32, u64, usize);
    ui.weak(format!("{value:?}"));
    false
}

/// A colour (sRGB, 0 to 1): a picker, and the three numbers.
fn colour_edit(ui: &mut egui::Ui, colour: &mut (f32, f32, f32)) -> bool {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut picked = [byte(colour.0), byte(colour.1), byte(colour.2)];
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.color_edit_button_srgb(&mut picked).changed() {
            let unit = |value: u8| (f32::from(value) / 255.0 * 100.0).round() / 100.0;
            *colour = (unit(picked[0]), unit(picked[1]), unit(picked[2]));
            changed = true;
        }
        for part in [&mut colour.0, &mut colour.1, &mut colour.2] {
            changed |= ui.add(egui::DragValue::new(part).speed(0.01).range(0.0..=1.0).max_decimals(2)).changed();
        }
    });
    changed
}

/// Dice as text ("1d6", "2d4+1"); text that isn't dice is shown in red and
/// not kept.
fn dice_edit(ui: &mut egui::Ui, dice: &mut Dice) -> bool {
    let id = ui.next_auto_id();
    let mut text = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_else(|| dice.to_string());
    let valid = text.parse::<Dice>();
    let edit = egui::TextEdit::singleline(&mut text).desired_width(90.0);
    let edit = if valid.is_err() { edit.text_color(egui::Color32::LIGHT_RED) } else { edit };
    let response = ui.add(edit);
    let mut changed = false;
    if response.changed() {
        if let Ok(parsed) = text.parse::<Dice>() {
            changed = parsed != *dice;
            *dice = parsed;
        }
        ui.data_mut(|data| data.insert_temp(id, text.clone()));
    }
    if !response.has_focus() {
        ui.data_mut(|data| data.remove::<String>(id));
    }
    if let Ok(dice) = text.parse::<Dice>() {
        let (low, high) = dice.range();
        response.on_hover_text(format!("{low} to {high}, {} on average", dice.average()));
    }
    changed
}

/// A field or kind name made readable: "armour_class" is "Armour class",
/// "RespawnPoint" is "Respawn point".
pub fn readable(name: &str) -> String {
    let mut words = String::new();
    for (index, letter) in name.chars().enumerate() {
        if letter == '_' {
            words.push(' ');
        } else if letter.is_uppercase() && index > 0 {
            words.push(' ');
            words.extend(letter.to_lowercase());
        } else if index == 0 {
            words.extend(letter.to_uppercase());
        } else {
            words.push(letter);
        }
    }
    words
}

/// A blank value of a type: the type's default if it has one; otherwise
/// built from its parts' blanks.
fn default_for(info: &'static TypeInfo, registry: &TypeRegistry) -> Option<Box<dyn PartialReflect>> {
    if let Some(default) = registry.get_type_data::<ReflectDefault>(info.type_id()) {
        return Some(default.default().into_partial_reflect());
    }
    let info_of = |type_id| registry.get_type_info(type_id);
    match info {
        TypeInfo::Struct(record) => {
            let mut built = DynamicStruct::default();
            built.set_represented_type(Some(info));
            for field in record.iter() {
                built.insert_boxed(field.name(), default_for(info_of(field.type_id())?, registry)?);
            }
            Some(Box::new(built))
        }
        TypeInfo::Tuple(tuple) => {
            let mut built = DynamicTuple::default();
            built.set_represented_type(Some(info));
            for field in tuple.iter() {
                built.insert_boxed(default_for(info_of(field.type_id())?, registry)?);
            }
            Some(Box::new(built))
        }
        TypeInfo::List(_) => {
            let mut built = DynamicList::default();
            built.set_represented_type(Some(info));
            Some(Box::new(built))
        }
        TypeInfo::Enum(choice) => {
            let variant =
                choice.iter().find(|variant| matches!(variant, VariantInfo::Unit(_))).or(choice.variant_at(0))?;
            let mut built = default_variant(variant, registry)?;
            built.set_represented_type(Some(info));
            Some(Box::new(built))
        }
        _ => None,
    }
}

/// One kind of an enum, with blank parts.
fn default_variant(variant: &'static VariantInfo, registry: &TypeRegistry) -> Option<DynamicEnum> {
    let info_of = |type_id| registry.get_type_info(type_id);
    let body = match variant {
        VariantInfo::Unit(_) => DynamicVariant::Unit,
        VariantInfo::Tuple(tuple) => {
            let mut built = DynamicTuple::default();
            for field in tuple.iter() {
                built.insert_boxed(default_for(info_of(field.type_id())?, registry)?);
            }
            DynamicVariant::Tuple(built)
        }
        VariantInfo::Struct(record) => {
            let mut built = DynamicStruct::default();
            for field in record.iter() {
                built.insert_boxed(field.name(), default_for(info_of(field.type_id())?, registry)?);
            }
            DynamicVariant::Struct(built)
        }
    };
    Some(DynamicEnum::new(variant.name(), body))
}

/// Calls `visit` with every reference a value holds (its kind, and the name,
/// to read or change).
pub fn visit_references(
    value: &mut dyn PartialReflect,
    refers: Option<Refers>,
    visit: &mut dyn FnMut(Refers, &mut String),
) {
    if let Some(kind) = refers {
        if let Some(text) = value.try_downcast_mut::<String>() {
            visit(kind, text);
            return;
        }
        if let Some(Some(text)) = value.try_downcast_mut::<Option<String>>() {
            visit(kind, text);
            return;
        }
    }
    match value.reflect_mut() {
        ReflectMut::Struct(record) => {
            let info = match record.get_represented_type_info() {
                Some(TypeInfo::Struct(info)) => Some(info),
                _ => None,
            };
            for index in 0..record.field_len() {
                let refers = info.and_then(|info| info.field_at(index)?.get_attribute::<Refers>()).copied();
                if let Some(field) = record.field_at_mut(index) {
                    visit_references(field, refers, visit);
                }
            }
        }
        ReflectMut::List(list) => {
            for index in 0..list.len() {
                if let Some(item) = list.get_mut(index) {
                    visit_references(item, refers, visit);
                }
            }
        }
        ReflectMut::Enum(choice) => {
            for index in 0..choice.field_len() {
                if let Some(field) = choice.field_at_mut(index) {
                    visit_references(field, refers, visit);
                }
            }
        }
        ReflectMut::Tuple(tuple) => {
            for index in 0..tuple.field_len() {
                if let Some(field) = tuple.field_mut(index) {
                    visit_references(field, refers, visit);
                }
            }
        }
        ReflectMut::TupleStruct(tuple) => {
            for index in 0..tuple.field_len() {
                if let Some(field) = tuple.field_mut(index) {
                    visit_references(field, refers, visit);
                }
            }
        }
        _ => {}
    }
}

/// What entries of a kind of content are called when referred to, if they are.
pub fn referred_as(title: &str) -> Option<Refers> {
    match title {
        "Appearances" => Some(Refers::Appearance),
        "Monsters" => Some(Refers::Monster),
        "Items" => Some(Refers::Item),
        "Abilities" => Some(Refers::Ability),
        _ => None,
    }
}

/// Everything referring to a name: (collection, file, entry), with a label
/// ("NPCs: town_guard").
pub fn users(
    document: &mut crate::document::Document,
    kind: Refers,
    name: &str,
) -> Vec<((usize, usize, usize), String)> {
    let mut found = Vec::new();
    for c in 0..document.collections.len() {
        let collection = &mut document.collections[c];
        let title = collection.title();
        for f in 0..collection.paths().len() {
            let keys = collection.keys(f);
            for (i, key) in keys.iter().enumerate() {
                let Some(entry) = collection.entry_mut(f, i) else { continue };
                let mut uses = false;
                visit_references(entry, None, &mut |refers, text| uses |= refers == kind && text == name);
                if uses {
                    found.push(((c, f, i), format!("{title}: {key}")));
                }
            }
        }
    }
    found
}

/// Renames an entry, and every reference to it. `now` is for undo.
pub fn rename_everywhere(
    document: &mut crate::document::Document,
    (c, f, i): (usize, usize, usize),
    new: &str,
    now: f64,
) {
    let Some(old) = document.collections[c].keys(f).get(i).cloned() else { return };
    let kind = referred_as(document.collections[c].title());
    document.collections[c].rename(f, i, new.to_string());
    document.changed(c, f, now);
    let Some(kind) = kind else { return };
    for c in 0..document.collections.len() {
        for f in 0..document.collections[c].paths().len() {
            let mut touched = false;
            for i in 0..document.collections[c].keys(f).len() {
                let Some(entry) = document.collections[c].entry_mut(f, i) else { continue };
                visit_references(entry, None, &mut |refers, text| {
                    if refers == kind && *text == old {
                        *text = new.to_string();
                        touched = true;
                    }
                });
            }
            if touched {
                document.changed(c, f, now);
            }
        }
    }
}

/// Keeps [`Choices`] up to date.
pub struct FieldsPlugin;

impl Plugin for FieldsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Choices>().add_systems(Update, update_choices);
    }
}

/// What each kind of reference can be: ids from the document, files from the
/// content folder, and parts and hair from the character models.
fn update_choices(
    document: Res<crate::document::Document>,
    project: Res<crate::Project>,
    store: Option<Res<venture_client::content::ContentStore>>,
    mut choices: ResMut<Choices>,
    mut seen: Local<Option<u64>>,
) {
    if *seen == Some(document.revision) && !store.as_ref().is_some_and(|store| store.is_changed()) {
        return;
    }
    *seen = Some(document.revision);
    let keys = |title: &str| -> Vec<String> {
        let Some(kind) = document.collections.iter().find(|kind| kind.title() == title) else { return Vec::new() };
        let mut keys: Vec<String> = (0..kind.paths().len()).flat_map(|file| kind.keys(file)).collect();
        keys.sort();
        keys
    };
    let files = |folder: &str, extension: &str| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(project.content.join(folder))
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .filter(|name| name.ends_with(extension))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    };
    let models = store.as_ref().and_then(|store| store.character_models.as_ref());
    let mut map = HashMap::new();
    map.insert(Refers::Appearance, keys("Appearances"));
    map.insert(Refers::Monster, keys("Monsters"));
    map.insert(Refers::Item, keys("Items"));
    map.insert(Refers::Ability, keys("Abilities"));
    map.insert(Refers::Icon, files("icons", ".png"));
    map.insert(Refers::Sound, files("sounds", ".wav"));
    map.insert(Refers::Script, files("scripts", ".rhai"));
    map.insert(Refers::Part, models.map(|models| models.parts.keys().cloned().collect()).unwrap_or_default());
    // (Hairstyles are all the hair that isn't a beard, so NPCs can have
    // styles players can't choose.)
    let beards = models.map(|models| models.beards.clone()).unwrap_or_default();
    let hairstyles = models.map(|models| models.hair.keys().filter(|hair| !beards.contains(hair)).cloned().collect());
    map.insert(Refers::Hairstyle, hairstyles.unwrap_or_default());
    map.insert(Refers::Beard, beards);
    map.insert(
        Refers::MonsterModel,
        models.map(|models| models.monsters.keys().cloned().collect()).unwrap_or_default(),
    );
    map.insert(Refers::Bone, venture_content::characters::BONES.iter().map(|bone| bone.to_string()).collect());
    choices.0 = map;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_heal_starts_with_a_sensible_amount() {
        use venture_content::{ItemDef, ItemEffect};
        let mut registry = TypeRegistry::default();
        registry.register::<ItemDef>();
        let Some(TypeInfo::Enum(effects)) = registry.get_type_info(std::any::TypeId::of::<ItemEffect>()) else {
            panic!("item effects are an enum");
        };
        let heal = default_variant(effects.variant("Heal").unwrap(), &registry).unwrap();
        let mut effect = ItemEffect::Nothing;
        effect.apply(&heal);
        assert_eq!(effect, ItemEffect::Heal(venture_rpg::Healing::Flat(5)));
    }

    #[test]
    fn renames_follow_every_reference_and_undo_together() {
        use std::path::Path;
        let folder = std::env::temp_dir().join(format!("venture-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        crate::document::copy_folder(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"), &folder).unwrap();
        let mut document = crate::document::Document::load(&folder).unwrap();
        let items = document.collections.iter().position(|kind| kind.title() == "Items").unwrap();
        let bread = document.collections[items].keys(0).iter().position(|key| key == "bread").unwrap();
        let before = users(&mut document, Refers::Item, "bread").len();
        assert!(before >= 2, "bread is sold and given to new characters");

        rename_everywhere(&mut document, (items, 0, bread), "loaf", 5.0);
        assert_eq!(users(&mut document, Refers::Item, "loaf").len(), before);
        assert!(users(&mut document, Refers::Item, "bread").is_empty());
        let mirror = folder.with_extension("mirror");
        crate::document::copy_folder(&folder, &mirror).unwrap();
        assert!(document.check(&mirror).is_empty(), "{:?}", document.check(&mirror));

        document.undo();
        assert_eq!(users(&mut document, Refers::Item, "bread").len(), before);
        assert_eq!(document.collections[items].keys(0)[bread], "bread");
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&mirror);
    }

    #[test]
    fn names_are_readable() {
        assert_eq!(readable("armour_class"), "Armour class");
        assert_eq!(readable("RespawnPoint"), "Respawn point");
        assert_eq!(readable("id"), "Id");
    }
}
