#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Stage {
    pub operation: u32,
    pub argument1: u32,
    pub argument2: u32,
    pub coordinate: u32,
}

impl Stage {
    pub fn defaults() -> [Self; 8] {
        std::array::from_fn(|index| Self {
            operation: if index == 0 { 4 } else { 1 },
            argument1: 2,
            argument2: 1,
            coordinate: u32::try_from(index).expect("eight texture stages"),
        })
    }

    pub fn set(&mut self, index: usize, kind: u32, value: u32) -> bool {
        match kind {
            1 if (1..=5).contains(&value) && (index < 2 || value == 1) => {
                self.operation = value;
            }
            2 if value <= 2 => self.argument1 = value,
            3 if value <= 2 => self.argument2 = value,
            11 if value <= 7 => self.coordinate = value,
            _ => return false,
        }
        true
    }

    pub fn get(self, kind: u32) -> Option<u32> {
        match kind {
            1 => Some(self.operation),
            2 => Some(self.argument1),
            3 => Some(self.argument2),
            11 => Some(self.coordinate),
            _ => None,
        }
    }

    pub fn uses_texture(self) -> bool {
        (self.operation != 3 && self.argument1 == 2) || (self.operation != 2 && self.argument2 == 2)
    }

    pub fn combine(self, diffuse: [f64; 3], current: [f64; 3], texture: [u8; 3]) -> [f64; 3] {
        let argument = |kind| match kind {
            0 => diffuse,
            1 => current,
            2 => texture.map(f64::from),
            _ => unreachable!("validated color argument"),
        };
        let a = argument(self.argument1);
        let b = argument(self.argument2);
        std::array::from_fn(|channel| {
            match self.operation {
                2 => a[channel],
                3 => b[channel],
                4 => a[channel] * b[channel] / 255.0,
                5 => 2.0 * a[channel] * b[channel] / 255.0,
                _ => unreachable!("validated active color operation"),
            }
            .clamp(0.0, 255.0)
        })
    }
}
