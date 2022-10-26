use std::fmt::Display;

use anyhow::anyhow;
use anyhow::Result;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GameGenieCode {
    pub address: u16,
    pub compare: Option<u8>,
    pub value: u8,
}

fn genie_nibble_to_char(byte: u8) -> char {
    match byte & 0xf {
        0x0 => 'A',
        0x1 => 'P',
        0x2 => 'Z',
        0x3 => 'L',
        0x4 => 'G',
        0x5 => 'I',
        0x6 => 'T',
        0x7 => 'Y',
        0x8 => 'E',
        0x9 => 'O',
        0xA => 'X',
        0xB => 'U',
        0xC => 'K',
        0xD => 'S',
        0xE => 'V',
        0xF => 'N',
        _ => unreachable!(),
    }
}
impl Display for GameGenieCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(compare) = self.compare {
            // 8 character code
            let off = self.address - 0x8000;

            // Offset nibble mapping: _333 4555 1222 3444
            // Value nibble mapping: 0111 7000
            // Compare nibble mapping 6777 5666
            let n0 = self.value & 0b0000_0111 | (self.value & 0b1000_0000) >> 4;
            let n1 = (self.value & 0b0111_0000) >> 4 | ((off as u8) & 0b1000_0000) >> 4;
            let n2 = (off & 0b0000_0000_0111_0000) >> 4;
            let n3 = (off & 0b0111_0000_0000_0000) >> 12 | off & 0b0000_0000_0000_1000;
            let n4 = off & 0b0000_0000_0000_0111 | (off & 0b0000_1000_0000_0000) >> 8;
            let n5 = (off & 0b0000_0111_0000_0000) >> 8 | (compare as u16) & 0b0000_0000_0000_1000;
            let n6 = compare & 0b0000_0111 | (compare & 0b1000_0000) >> 4;
            let n7 = (compare & 0b0111_0000) >> 4 | self.value & 0b0000_1000;

            write!(
                f,
                "{}{}{}{}{}{}{}{}",
                genie_nibble_to_char(n0),
                genie_nibble_to_char(n1),
                genie_nibble_to_char(n2 as u8),
                genie_nibble_to_char(n3 as u8),
                genie_nibble_to_char(n4 as u8),
                genie_nibble_to_char(n5 as u8),
                genie_nibble_to_char(n6 as u8),
                genie_nibble_to_char(n7 as u8)
            )
        } else {
            // 6 character code
            let off = self.address - 0x8000;

            // Offset nibble mapping: _333 4555 1222 3444
            // Value nibble mapping:  0111 5000
            let n0 = self.value & 0b0000_0111 | (self.value & 0b1000_0000) >> 4;
            let n1 = (self.value & 0b0111_0000) >> 4 | ((off as u8) & 0b1000_0000) >> 4;
            let n2 = (off & 0b0000_0000_0111_0000) >> 4 | (off & 0b1000_0000_0000_0000) >> 12;
            let n3 = (off & 0b0111_0000_0000_0000) >> 12 | off & 0b0000_0000_0000_1000;
            let n4 = off & 0b0000_0000_0000_0111 | (off & 0b0000_1000_0000_0000) >> 8;
            let n5 =
                (off & 0b0000_0111_0000_0000) >> 8 | (self.value as u16) & 0b0000_0000_0000_1000;

            write!(
                f,
                "{}{}{}{}{}{}",
                genie_nibble_to_char(n0),
                genie_nibble_to_char(n1),
                genie_nibble_to_char(n2 as u8),
                genie_nibble_to_char(n3 as u8),
                genie_nibble_to_char(n4 as u8),
                genie_nibble_to_char(n5 as u8)
            )
        }
    }
}

impl TryFrom<&str> for GameGenieCode {
    type Error = anyhow::Error;

    fn try_from(code: &str) -> Result<Self, Self::Error> {
        let nibbles: Result<Vec<u8>> = code
            .chars()
            .map(|c| match c {
                'A' => Ok(0x0),
                'P' => Ok(0x1),
                'Z' => Ok(0x2),
                'L' => Ok(0x3),
                'G' => Ok(0x4),
                'I' => Ok(0x5),
                'T' => Ok(0x6),
                'Y' => Ok(0x7),
                'E' => Ok(0x8),
                'O' => Ok(0x9),
                'X' => Ok(0xA),
                'U' => Ok(0xB),
                'K' => Ok(0xC),
                'S' => Ok(0xD),
                'V' => Ok(0xE),
                'N' => Ok(0xF),
                _ => Err(anyhow!("Invalid game genie code: {code}")),
            })
            .collect();

        let nibbles = nibbles?;

        match nibbles.len() {
            6 => {
                // _333 4555 1222 3444
                let address = 0x8000u16 + ((u16::from(nibbles[3]) & 7) << 12)
                    | ((u16::from(nibbles[5]) & 7) << 8)
                    | ((u16::from(nibbles[4]) & 8) << 8)
                    | ((u16::from(nibbles[2]) & 7) << 4)
                    | ((u16::from(nibbles[1]) & 8) << 4)
                    | (u16::from(nibbles[4]) & 7)
                    | (u16::from(nibbles[3]) & 8);

                // 0111 5000
                let value = ((nibbles[1] & 7) << 4)
                    | ((nibbles[0] & 8) << 4)
                    | (nibbles[0] & 7)
                    | (nibbles[5] & 8);

                Ok(GameGenieCode {
                    address,
                    compare: None,
                    value,
                })
            }
            8 => {
                // _333 4555 1222 3444
                let address = 0x8000u16 + ((u16::from(nibbles[3]) & 7) << 12)
                    | ((u16::from(nibbles[5]) & 7) << 8)
                    | ((u16::from(nibbles[4]) & 8) << 8)
                    | ((u16::from(nibbles[2]) & 7) << 4)
                    | ((u16::from(nibbles[1]) & 8) << 4)
                    | (u16::from(nibbles[4]) & 7)
                    | (u16::from(nibbles[3]) & 8);
                // 0111 7000
                let value = ((nibbles[1] & 7) << 4)
                    | ((nibbles[0] & 8) << 4)
                    | (nibbles[0] & 7)
                    | (nibbles[7] & 8);

                // 6777 5666
                let compare = ((nibbles[7] & 7) << 4)
                    | ((nibbles[6] & 8) << 4)
                    | (nibbles[6] & 7)
                    | (nibbles[5] & 8);

                Ok(GameGenieCode {
                    address,
                    compare: Some(compare),
                    value,
                })
            }
            _ => Err(anyhow!(
                "Invalid Game Genie code {code}; should be 6 or 8 characters long"
            )),
        }
    }
}

#[test]
fn test_game_genie_parse() {
    let code: GameGenieCode = "ZEXPYGLA".try_into().unwrap();

    debug_assert_eq!(
        code,
        GameGenieCode {
            address: 0x94A7,
            value: 0x02,
            compare: Some(0x03),
        }
    );
}

#[test]
fn test_game_genie_encode() {
    // Note: it's not enough to check that we get the same string
    // back because the top bit for nibble[2] is ignored and so the
    // third character may change

    // 8 character code
    let code: GameGenieCode = "ZEXPYGLA".try_into().unwrap();

    debug_assert_eq!(
        code,
        GameGenieCode {
            address: 0x94A7,
            value: 0x02,
            compare: Some(0x03),
        }
    );

    let code2: GameGenieCode = code.to_string().as_str().try_into().unwrap();
    debug_assert_eq!(code, code2);

    // 6 character code
    let code: GameGenieCode = "ZEXPYG".try_into().unwrap();
    let code2: GameGenieCode = code.to_string().as_str().try_into().unwrap();
    debug_assert_eq!(code, code2);
}
