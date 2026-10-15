use super::super::{Declaration, Program};

#[derive(Debug)]
pub struct VertexShader {
    pub declaration: Declaration,
    pub function: Option<Program>,
    pub usage: u32,
}

impl VertexShader {
    pub fn bind_constants(&self) -> impl Iterator<Item = (u32, [u32; 4])> + '_ {
        self.declaration
            .constants()
            .iter()
            .map(|(index, value)| (u32::from(*index), *value))
            .chain(
                self.function
                    .iter()
                    .flat_map(|program| definitions(program)),
            )
    }

    pub(super) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.declaration.retained_bytes()
            + self.function.as_ref().map_or(0, Program::retained_bytes)
    }
}

#[derive(Debug)]
pub struct PixelShader {
    pub function: Program,
}

impl PixelShader {
    pub fn bind_constants(&self) -> impl Iterator<Item = (u32, [u32; 4])> + '_ {
        definitions(&self.function)
    }
    pub(super) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.function.retained_bytes()
    }
}

fn definitions(program: &Program) -> impl Iterator<Item = (u32, [u32; 4])> + '_ {
    program
        .bind_constants()
        .map(|(index, value)| (u32::from(index), value))
}
