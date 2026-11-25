//! A place's collision groups, set up in Studio's collision groups editor.
//!
//! Newer files keep them on the workspace as `CollisionGroupData` (bytes),
//! older ones as `CollisionGroups` (text such as `Default^0^-1\Doors^1^-3`).
//! Older files also name each part's group by its number
//! (`CollisionGroupId`) rather than by name.

use rbx_dom_weak::WeakDom;
use rbx_dom_weak::types::Variant;

/// One group: its number, name, and which group numbers it collides with
/// (bit `n` set: it collides with group `n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Group {
    pub id: u32,
    pub name: String,
    pub mask: u32,
}

/// The groups in the file's workspace, if it says.
pub(crate) fn groups(dom: &WeakDom) -> Vec<Group> {
    let root = dom.root();
    let Some(ws) = root.children().iter().filter_map(|r| dom.get_by_ref(*r)).find(|i| i.class == "Workspace")
    else {
        return Vec::new();
    };
    let data = ws.properties.get(&"CollisionGroupData".into()).and_then(|v| match v {
        Variant::BinaryString(b) => from_bytes(b.as_ref()),
        _ => None,
    });
    let text = || {
        ws.properties.get(&"CollisionGroups".into()).and_then(|v| match v {
            Variant::String(s) => from_text(s),
            _ => None,
        })
    };
    data.or_else(text).unwrap_or_default()
}

/// `CollisionGroupData`: a version (1), a count, then for each group its
/// number, the size of its mask (4), the mask, and its name's length and name.
fn from_bytes(bytes: &[u8]) -> Option<Vec<Group>> {
    let (&version, rest) = bytes.split_first()?;
    if version != 1 {
        return None;
    }
    let (&count, mut rest) = rest.split_first()?;
    let mut out = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let (&id, r) = rest.split_first()?;
        let (&size, r) = r.split_first()?;
        if size != 4 || r.len() < 5 {
            return None;
        }
        let mask = u32::from_le_bytes(r[..4].try_into().ok()?);
        let len = usize::from(r[4]);
        let name = std::str::from_utf8(r.get(5..5 + len)?).ok()?;
        out.push(Group { id: u32::from(id), name: name.to_owned(), mask });
        rest = &r[5 + len..];
    }
    rest.is_empty().then_some(out)
}

/// `CollisionGroups`: `name^number^mask` entries separated by `\`.
pub(crate) fn from_text(text: &str) -> Option<Vec<Group>> {
    let mut out = Vec::new();
    for entry in text.split('\\').filter(|e| !e.is_empty()) {
        let mut parts = entry.rsplitn(3, '^');
        let mask = parts.next()?.trim().parse::<i64>().ok()? as u32;
        let id = parts.next()?.trim().parse::<u32>().ok()?;
        let name = parts.next()?;
        out.push(Group { id, name: name.to_owned(), mask });
    }
    Some(out)
}

/// The groups as `CollisionGroups` text, for scripts' physics service.
pub(crate) fn to_text(groups: &[Group]) -> String {
    groups.iter().map(|g| format!("{}^{}^{}", g.name, g.id, g.mask as i32)).collect::<Vec<_>>().join("\\")
}

/// Whether any pair of groups passes through each other, or there's a
/// group beyond the default one: worth telling the game about.
pub(crate) fn matter(groups: &[Group]) -> bool {
    groups.len() > 1 || groups.iter().any(|g| groups.iter().any(|o| g.mask & (1 << (o.id & 31)) == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_group_data_reads() {
        // A new baseplate's: just the default group, colliding with everything.
        let bytes = [1, 1, 0, 4, 255, 255, 255, 255, 7, b'D', b'e', b'f', b'a', b'u', b'l', b't'];
        let groups = from_bytes(&bytes).unwrap();
        assert_eq!(groups, vec![Group { id: 0, name: "Default".into(), mask: u32::MAX }]);
        assert!(!matter(&groups));
        assert!(from_bytes(&bytes[..10]).is_none());
    }

    #[test]
    fn older_group_text_reads_and_writes() {
        let groups = from_text("Default^0^-1\\Ghosts^1^-3").unwrap();
        assert_eq!(groups[1], Group { id: 1, name: "Ghosts".into(), mask: (-3i32) as u32 });
        assert!(matter(&groups));
        assert_eq!(to_text(&groups), "Default^0^-1\\Ghosts^1^-3");
    }
}
