//! Properties the file readers renamed on the way in.
//!
//! The rbx-dom readers move properties that have been replaced onto their new
//! names as they read (a sound's `SoundId` arrives as `AudioContent`, a
//! text label's `Font` as `FontFace`, a corner's `CornerRadius` as four
//! radii). PlayLibre knows most of them by the older names, so this looks
//! each new name up in the reflection database's own list of those moves
//! and gives back the older one.

type HashMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::RandomState>;
use std::sync::OnceLock;

use rbx_dom_weak::types::{Enum, Font, Variant};
use rbx_reflection::{PropertyKind, PropertySerialization, ReflectionDatabase};

/// What the reader did to a property that arrives under `new` on a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moved {
    /// A picture, sound or mesh address: the older name, same address.
    Address(&'static str),
    /// A text font (`FontFace`): the older font list's name for it.
    Font(&'static str),
    /// One of a corner's four radii; the older single radius.
    Corner(&'static str),
    /// A screen's insets; the older on-or-off setting.
    Insets(&'static str),
}

struct Table {
    /// Each class with moved properties (its own or ones it inherits), and
    /// each new name to what it was.
    moved: HashMap<&'static str, Moves>,
    /// Each older font's face, to find a face's older font.
    fonts: Vec<(Font, u32)>,
}

fn table(db: &'static ReflectionDatabase<'static>) -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut own: HashMap<&'static str, Moves> = HashMap::default();
        let mut font_migration = None;
        for (class_name, class) in &db.classes {
            for (old, descriptor) in &class.properties {
                let PropertyKind::Canonical { serialization: PropertySerialization::Migrate(m) } =
                    &descriptor.kind
                else {
                    continue;
                };
                let names = m.new_property_names();
                let old: &'static str = old;
                let class_name: &'static str = class_name;
                for &new in names {
                    use rbx_dom_weak::types::VariantType;
                    use rbx_reflection::DataType;
                    let kind = match &descriptor.data_type {
                        DataType::Value(VariantType::ContentId | VariantType::Int64) => Moved::Address(old),
                        DataType::Enum("Font") => {
                            font_migration.get_or_insert(m);
                            Moved::Font(old)
                        }
                        DataType::Value(VariantType::UDim) => Moved::Corner(old),
                        DataType::Value(VariantType::Bool) => Moved::Insets(old),
                        // A part's brick colour arrives as its colour, which PlayLibre uses.
                        _ => continue,
                    };
                    own.entry(class_name).or_default().insert(new, kind);
                }
            }
        }
        // Every class gets the moves of the classes it comes from.
        let mut moved = HashMap::default();
        for &name in db.classes.keys() {
            let mut all = Moves::default();
            let mut current = db.classes.get(name);
            while let Some(c) = current {
                for (k, v) in own.get(c.name).into_iter().flatten() {
                    all.entry(*k).or_insert(*v);
                }
                current = c.superclass.and_then(|s| db.classes.get(s));
            }
            if !all.is_empty() {
                moved.insert(name, all);
            }
        }
        let mut fonts = Vec::new();
        if let (Some(m), Some(list)) = (font_migration, db.enums.get("Font")) {
            let mut items: Vec<u32> = list.items.values().copied().collect();
            items.sort_unstable();
            for n in items {
                if let Ok(Variant::Font(face)) = m.perform(&Variant::Enum(Enum::from_u32(n))) {
                    fonts.push((face, n));
                }
            }
        }
        Table { moved, fonts }
    })
}

/// The properties of `class` that were moved on the way in, by new name;
/// None for the many classes with none (looked up once per instance).
pub(crate) fn for_class(db: &'static ReflectionDatabase<'static>, class: &str) -> Option<&'static Moves> {
    table(db).moved.get(class)
}

/// A class's moved properties, by new name.
pub(crate) type Moves = HashMap<&'static str, Moved>;

/// The older font list's number for a face, when it is one of theirs.
pub(crate) fn font_number(db: &'static ReflectionDatabase<'static>, face: &Font) -> Option<u32> {
    let fonts = &table(db).fonts;
    let same = |f: &Font| f.family.eq_ignore_ascii_case(&face.family);
    fonts
        .iter()
        .find(|(f, _)| same(f) && f.weight == face.weight && f.style == face.style)
        .or_else(|| fonts.iter().find(|(f, _)| same(f)))
        .map(|(_, n)| *n)
}
