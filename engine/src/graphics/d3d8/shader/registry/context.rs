use std::sync::{Arc, Weak};

use super::super::{Declaration, ElementType, Program, Stage};
use super::{PixelShader, RegistryError, ShaderLimits, VertexShader, is_shader_handle};

pub struct ShaderRegistry {
    limits: ShaderLimits,
    entries: Vec<Entry>,
    retired: Vec<Retired>,
    retained_bytes: usize,
    next_sequence: u32,
}

struct Entry {
    handle: u32,
    shader: Shader,
    bytes: usize,
}

pub struct PreparedShader {
    entry: Entry,
}

impl PreparedShader {
    pub fn handle(&self) -> u32 {
        self.entry.handle
    }
    pub fn vertex(&self) -> Option<&VertexShader> {
        match &self.entry.shader {
            Shader::Vertex(shader) => Some(shader),
            _ => None,
        }
    }
    pub fn pixel(&self) -> Option<&PixelShader> {
        match &self.entry.shader {
            Shader::Pixel(shader) => Some(shader),
            _ => None,
        }
    }
}
enum Shader {
    Vertex(Arc<VertexShader>),
    Pixel(Arc<PixelShader>),
}
struct Retired {
    shader: RetiredShader,
    bytes: usize,
}
enum RetiredShader {
    Vertex(Weak<VertexShader>),
    Pixel(Weak<PixelShader>),
}

impl ShaderRegistry {
    #[cfg(test)]
    pub(super) fn next_sequence_for_test(&mut self, value: u32) {
        self.next_sequence = value;
    }
    pub fn new(limits: ShaderLimits) -> Result<Self, RegistryError> {
        if limits.objects == 0
            || limits.objects > 4096
            || limits.bytes == 0
            || limits.bytes > 64 * 1024 * 1024
        {
            return Err(RegistryError::InvalidLimits);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(limits.objects as usize)
            .map_err(|_| RegistryError::Allocation)?;
        let mut retired = Vec::new();
        retired
            .try_reserve_exact(limits.objects as usize)
            .map_err(|_| RegistryError::Allocation)?;
        Ok(Self {
            limits,
            entries,
            retired,
            retained_bytes: 0,
            next_sequence: 0,
        })
    }

    pub fn create_vertex(
        &mut self,
        declaration: &[u32],
        function: Option<&[u32]>,
        usage: u32,
    ) -> Result<u32, RegistryError> {
        let prepared = self.prepare_vertex(declaration, function, usage)?;
        self.can_commit(&prepared)?;
        Ok(self.commit(prepared))
    }

    pub fn prepare_vertex(
        &mut self,
        declaration: &[u32],
        function: Option<&[u32]>,
        usage: u32,
    ) -> Result<PreparedShader, RegistryError> {
        if usage & !0x10 != 0 {
            return Err(RegistryError::InvalidUsage);
        }
        let declaration = Declaration::parse(declaration)?;
        let function = function
            .map(|words| Program::parse(words, Stage::Vertex))
            .transpose()?;
        if let Some(function) = &function {
            function.validate_declaration(&declaration)?;
        } else if declaration
            .elements()
            .iter()
            .any(|element| element.register == 3 && element.kind != ElementType::Float3)
        {
            return Err(super::super::ShaderError::InvalidDeclaration.into());
        }
        let shader = VertexShader {
            declaration,
            function,
            usage,
        };
        let bytes = shader.retained_bytes();
        let handle = self.reserve(bytes)?;
        Ok(PreparedShader {
            entry: Entry {
                handle,
                shader: Shader::Vertex(Arc::new(shader)),
                bytes,
            },
        })
    }

    pub fn create_pixel(&mut self, function: &[u32]) -> Result<u32, RegistryError> {
        let prepared = self.prepare_pixel(function)?;
        self.can_commit(&prepared)?;
        Ok(self.commit(prepared))
    }

    pub fn prepare_pixel(&mut self, function: &[u32]) -> Result<PreparedShader, RegistryError> {
        let shader = PixelShader {
            function: Program::parse(function, Stage::Pixel)?,
        };
        let bytes = shader.retained_bytes();
        let handle = self.reserve(bytes)?;
        Ok(PreparedShader {
            entry: Entry {
                handle,
                shader: Shader::Pixel(Arc::new(shader)),
                bytes,
            },
        })
    }

    pub fn can_commit(&self, prepared: &PreparedShader) -> Result<(), RegistryError> {
        if self.next_sequence > 0x3fff_ffff
            || prepared.entry.handle != (0x8000_0001 | self.next_sequence << 1)
        {
            return Err(RegistryError::InvalidHandle);
        }
        if self.entries.len() + self.retired.len() >= self.limits.objects as usize {
            return Err(RegistryError::ObjectCapacity);
        }
        if self
            .retained_bytes
            .checked_add(prepared.entry.bytes)
            .is_none_or(|total| total > self.limits.bytes as usize)
        {
            return Err(RegistryError::ByteCapacity);
        }
        Ok(())
    }

    pub fn commit(&mut self, prepared: PreparedShader) -> u32 {
        let handle = prepared.entry.handle;
        self.retained_bytes += prepared.entry.bytes;
        self.entries.push(prepared.entry);
        self.next_sequence += 1;
        handle
    }

    pub fn vertex(&self, handle: u32) -> Result<&Arc<VertexShader>, RegistryError> {
        match &self.entries[self.index(handle)?].shader {
            Shader::Vertex(shader) => Ok(shader),
            _ => Err(RegistryError::InvalidHandle),
        }
    }

    pub fn pixel(&self, handle: u32) -> Result<&Arc<PixelShader>, RegistryError> {
        match &self.entries[self.index(handle)?].shader {
            Shader::Pixel(shader) => Ok(shader),
            _ => Err(RegistryError::InvalidHandle),
        }
    }

    pub fn acquire_vertex(&self, handle: u32) -> Result<Arc<VertexShader>, RegistryError> {
        self.vertex(handle).map(Arc::clone)
    }
    pub fn acquire_pixel(&self, handle: u32) -> Result<Arc<PixelShader>, RegistryError> {
        self.pixel(handle).map(Arc::clone)
    }

    pub fn delete_vertex(&mut self, handle: u32) -> Result<Arc<VertexShader>, RegistryError> {
        let index = self.index(handle)?;
        if !matches!(self.entries[index].shader, Shader::Vertex(_)) {
            return Err(RegistryError::InvalidHandle);
        }
        let entry = self.entries.remove(index);
        let Shader::Vertex(shader) = entry.shader else {
            unreachable!()
        };
        self.retired.push(Retired {
            shader: RetiredShader::Vertex(Arc::downgrade(&shader)),
            bytes: entry.bytes,
        });
        Ok(shader)
    }

    pub fn delete_pixel(&mut self, handle: u32) -> Result<Arc<PixelShader>, RegistryError> {
        let index = self.index(handle)?;
        if !matches!(self.entries[index].shader, Shader::Pixel(_)) {
            return Err(RegistryError::InvalidHandle);
        }
        let entry = self.entries.remove(index);
        let Shader::Pixel(shader) = entry.shader else {
            unreachable!()
        };
        self.retired.push(Retired {
            shader: RetiredShader::Pixel(Arc::downgrade(&shader)),
            bytes: entry.bytes,
        });
        Ok(shader)
    }

    pub fn live_objects(&self) -> usize {
        self.entries.len()
    }

    pub fn retained_bytes(&mut self) -> usize {
        self.collect();
        self.retained_bytes
    }

    pub fn clear(&mut self) {
        for entry in self.entries.drain(..) {
            let shader = match entry.shader {
                Shader::Vertex(shader) => RetiredShader::Vertex(Arc::downgrade(&shader)),
                Shader::Pixel(shader) => RetiredShader::Pixel(Arc::downgrade(&shader)),
            };
            self.retired.push(Retired {
                shader,
                bytes: entry.bytes,
            });
        }
        self.collect();
    }

    fn reserve(&mut self, bytes: usize) -> Result<u32, RegistryError> {
        self.collect();
        if self.entries.len() + self.retired.len() == self.limits.objects as usize {
            return Err(RegistryError::ObjectCapacity);
        }
        if self
            .retained_bytes
            .checked_add(bytes)
            .is_none_or(|total| total > self.limits.bytes as usize)
        {
            return Err(RegistryError::ByteCapacity);
        }
        if self.next_sequence > 0x3fff_ffff {
            return Err(RegistryError::IdentityExhausted);
        }
        // bit zero is reserved in every fvf, including formats whose eighth texture size sets bit 31.
        Ok(0x8000_0001 | self.next_sequence << 1)
    }

    fn index(&self, handle: u32) -> Result<usize, RegistryError> {
        if !is_shader_handle(handle) {
            return Err(RegistryError::InvalidHandle);
        }
        self.entries
            .binary_search_by_key(&handle, |entry| entry.handle)
            .map_err(|_| RegistryError::InvalidHandle)
    }

    fn collect(&mut self) {
        self.retired.retain(|entry| {
            let retained = match &entry.shader {
                RetiredShader::Vertex(shader) => shader.strong_count() != 0,
                RetiredShader::Pixel(shader) => shader.strong_count() != 0,
            };
            if !retained {
                self.retained_bytes -= entry.bytes;
            }
            retained
        });
    }
}
