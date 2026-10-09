// Copyright (C) 2025 Windblows2000
// This file is part of nexa.
//
// nexa is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(crate) fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;

    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

pub(crate) fn stem_for_url(url: &str) -> String {
    format!("{:016x}", fnv1a_64(url.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{fnv1a_64, stem_for_url};

    #[test]
    fn fnv1a_64_matches_known_empty_value() {
        assert_eq!(fnv1a_64(b""), 0xcbf29ce484222325);
    }

    #[test]
    fn url_stem_has_sixteen_hex_characters() {
        let stem = stem_for_url("https://example.com/cover.jpg");

        assert_eq!(stem.len(), 16);
        assert!(stem.chars().all(|character| character.is_ascii_hexdigit()));
    }

    #[test]
    fn url_stem_is_deterministic() {
        let url = "https://example.com/cover.jpg";

        assert_eq!(stem_for_url(url), stem_for_url(url));
    }
}
