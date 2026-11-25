use libroxel_instance::InstanceId;

pub const MAX_CHAT: usize = 200;
const MAX_KEPT: usize = 64;
pub(crate) const PER_SECOND: f32 = 1.0;
pub(crate) const BURST: f32 = 5.0;

pub fn clean(text: &str) -> Option<String> {
    let line: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).take(MAX_CHAT).collect();
    let line = line.trim();
    (!line.is_empty()).then(|| libroxel_script::filter::filter(line))
}

#[derive(Default)]
pub(crate) struct Chat {
    pub(crate) out: Vec<String>,
    pub(crate) heard: Vec<(InstanceId, String)>,
    pub(crate) relay: Vec<(InstanceId, String)>,
}

impl Chat {
    pub(crate) fn hear(&mut self, from: InstanceId, text: String) {
        if self.heard.len() < MAX_KEPT {
            self.heard.push((from, text));
        }
    }

    pub(crate) fn say(&mut self, text: String) {
        if self.out.len() < MAX_KEPT {
            self.out.push(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_cleaned() {
        assert_eq!(clean("  hi there \n").as_deref(), Some("hi there"));
        assert_eq!(clean("what the fuck").as_deref(), Some("what the ####"));
        assert_eq!(clean("a\nb").as_deref(), Some("a b"));
        assert_eq!(clean(" \t "), None);
        assert_eq!(clean(&"x".repeat(500)).map(|s| s.len()), Some(MAX_CHAT));
    }
}
