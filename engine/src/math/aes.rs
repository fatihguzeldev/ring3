use aes::{Block, hazmat};

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    Encrypt,
    Decrypt,
}

pub(crate) fn round(state: [u8; 16], key: [u8; 16], direction: Direction, last: bool) -> [u8; 16] {
    let mut block = Block::from(state);
    let round_key = Block::from(if last { [0; 16] } else { key });
    match direction {
        Direction::Encrypt => {
            hazmat::cipher_round(&mut block, &round_key);
            if last {
                // removing mixcolumns from the full zero-key round leaves shiftrows(subbytes).
                hazmat::inv_mix_columns(&mut block);
            }
        }
        Direction::Decrypt => {
            hazmat::equiv_inv_cipher_round(&mut block, &round_key);
            if last {
                hazmat::mix_columns(&mut block);
            }
        }
    }
    let mut result: [u8; 16] = block.into();
    if last {
        for (byte, key) in result.iter_mut().zip(key) {
            *byte ^= key;
        }
    }
    result
}

pub(crate) fn inverse_mix_columns(source: [u8; 16]) -> [u8; 16] {
    let mut block = Block::from(source);
    hazmat::inv_mix_columns(&mut block);
    block.into()
}

pub(crate) fn key_assist(source: [u8; 16], constant: u8) -> [u8; 16] {
    let shifted = round(source, [0; 16], Direction::Encrypt, true);
    // aes stores columns consecutively; inverse shiftrows maps column c, row r to c-r.
    const UNSHIFT: [usize; 16] = [0, 13, 10, 7, 4, 1, 14, 11, 8, 5, 2, 15, 12, 9, 6, 3];
    let substituted: [u8; 16] = core::array::from_fn(|index| shifted[UNSHIFT[index]]);
    let mut result = [0; 16];
    for (source, destination) in [(4, 0), (12, 8)] {
        result[destination..destination + 4].copy_from_slice(&substituted[source..source + 4]);
        for index in 0..4 {
            result[destination + 4 + index] = substituted[source + (index + 1) % 4];
        }
        result[destination + 4] ^= constant;
    }
    result
}
