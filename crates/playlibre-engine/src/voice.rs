//! Voice chat: 16 kHz mono speech in 40 ms frames, packed 4:1 with IMA ADPCM.

use playlibre_instance::InstanceId;

pub const RATE: u32 = 16_000;
pub const FRAME: usize = 640;
pub(crate) const MAX_WIRE: usize = 4 * (3 + FRAME / 2).div_ceil(3);
const MAX_KEPT: usize = 64;

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88,
    97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658,
    724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660,
    4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818,
    18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const MOVES: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

#[derive(Clone, Copy)]
struct State {
    value: i32,
    index: i32,
}

impl State {
    fn apply(&mut self, code: u8) {
        let step = STEPS[self.index as usize];
        let mut diff = step >> 3;
        if code & 4 != 0 {
            diff += step;
        }
        if code & 2 != 0 {
            diff += step >> 1;
        }
        if code & 1 != 0 {
            diff += step >> 2;
        }
        self.value = if code & 8 != 0 { self.value - diff } else { self.value + diff }.clamp(-32768, 32767);
        self.index = (self.index + MOVES[(code & 7) as usize]).clamp(0, 88);
    }

    fn code(&mut self, sample: i16) -> u8 {
        let step = STEPS[self.index as usize];
        let mut diff = i32::from(sample) - self.value;
        let mut code = 0u8;
        if diff < 0 {
            code = 8;
            diff = -diff;
        }
        if diff >= step {
            code |= 4;
            diff -= step;
        }
        if diff >= step >> 1 {
            code |= 2;
            diff -= step >> 1;
        }
        if diff >= step >> 2 {
            code |= 1;
        }
        self.apply(code);
        code
    }
}

pub fn encode(samples: &[i16]) -> Vec<u8> {
    let first = samples.first().copied().unwrap_or(0);
    let mut state = State { value: i32::from(first), index: 0 };
    let mut out = Vec::with_capacity(3 + samples.len().div_ceil(2));
    out.extend_from_slice(&first.to_le_bytes());
    out.push(0);
    for pair in samples.chunks(2) {
        let low = state.code(pair[0]);
        let high = pair.get(1).map_or(0, |s| state.code(*s));
        out.push(low | (high << 4));
    }
    out
}

pub fn decode(bytes: &[u8]) -> Vec<i16> {
    let Some((head, body)) = bytes.split_first_chunk::<3>() else { return Vec::new() };
    let mut state =
        State { value: i32::from(i16::from_le_bytes([head[0], head[1]])), index: i32::from(head[2]).min(88) };
    let mut out = Vec::with_capacity(body.len() * 2);
    for byte in body.iter().take(FRAME / 2) {
        for code in [byte & 15, byte >> 4] {
            state.apply(code);
            out.push(state.value as i16);
        }
    }
    out
}

pub(crate) fn to_wire(samples: &[i16]) -> String {
    data_encoding::BASE64.encode(&encode(samples))
}

pub(crate) fn from_wire(text: &str) -> Option<Vec<i16>> {
    if text.len() > MAX_WIRE {
        return None;
    }
    let bytes = data_encoding::BASE64.decode(text.as_bytes()).ok()?;
    let samples = decode(&bytes);
    (!samples.is_empty()).then_some(samples)
}

#[derive(Default)]
pub(crate) struct Voice {
    pub(crate) out: Vec<String>,
    pub(crate) heard: Vec<(InstanceId, Vec<i16>)>,
    pub(crate) relay: Vec<(InstanceId, String)>,
}

impl Voice {
    pub(crate) fn hear(&mut self, from: InstanceId, samples: Vec<i16>) {
        if self.heard.len() < MAX_KEPT {
            self.heard.push((from, samples));
        }
    }

    pub(crate) fn say(&mut self, wire: String) {
        if self.out.len() < MAX_KEPT {
            self.out.push(wire);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_comes_back_close() {
        let tone: Vec<i16> = (0..FRAME)
            .map(|i| ((i as f32 * 0.07).sin() * 9000.0 + (i as f32 * 0.31).sin() * 3000.0) as i16)
            .collect();
        let wire = to_wire(&tone);
        assert!(wire.len() <= MAX_WIRE);
        let back = from_wire(&wire).unwrap();
        assert_eq!(back.len(), FRAME);
        let err: f64 = tone
            .iter()
            .zip(&back)
            .map(|(a, b)| f64::from(i32::from(*a) - i32::from(*b)).powi(2))
            .sum::<f64>()
            / FRAME as f64;
        assert!(err.sqrt() < 600.0, "{}", err.sqrt());
    }

    #[test]
    fn junk_is_refused() {
        assert!(from_wire("not base64!").is_none());
        assert!(from_wire("").is_none());
        assert!(from_wire(&"A".repeat(MAX_WIRE + 4)).is_none());
    }
}
