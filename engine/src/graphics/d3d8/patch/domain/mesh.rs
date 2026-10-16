use super::super::PatchError;
use super::{DomainKey, EdgeStyle, MAX_INDICES, MAX_VERTICES, partition};

pub struct DomainMesh {
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
    boundary: usize,
    rings: usize,
    key: Option<DomainKey>,
}

impl DomainMesh {
    pub fn new() -> Result<Self, PatchError> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        vertices
            .try_reserve_exact(MAX_VERTICES)
            .map_err(|_| PatchError)?;
        indices
            .try_reserve_exact(MAX_INDICES)
            .map_err(|_| PatchError)?;
        Ok(Self {
            vertices,
            indices,
            boundary: 0,
            rings: 0,
            key: None,
        })
    }
    pub fn key(&self) -> Option<DomainKey> {
        self.key
    }
    pub fn vertices(&self) -> &[[f32; 2]] {
        &self.vertices
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    pub fn boundary_count(&self) -> usize {
        self.boundary
    }
    pub fn rings(&self) -> usize {
        self.rings
    }
    pub fn rebuild(&mut self, key: DomainKey) -> Result<(), PatchError> {
        key.validate()?;
        let factors = key.segments.words().map(f32::from_bits);
        let edges = if key.triangular { 3 } else { 4 };
        let counts = factors.map(|f| {
            if f == 0.0 {
                0
            } else {
                partition::count(f, key.style)
            }
        });
        let boundary = counts[..edges].iter().sum::<u32>() as usize;
        let max = factors[..edges].iter().copied().fold(1.0, f32::max);
        let rings = (max * 0.5).ceil() as usize;
        let vertex_count = 1 + boundary * rings;
        let index_count = 3 * boundary * (2 * rings - 1);
        if vertex_count > MAX_VERTICES || index_count > MAX_INDICES {
            return Err(PatchError);
        }
        self.vertices.clear();
        self.indices.clear();
        self.vertices.resize(vertex_count, [0.0; 2]);
        let center = if key.triangular {
            [1.0 / 3.0; 2]
        } else {
            [0.5; 2]
        };
        self.vertices[0] = center;
        let start = 1 + (rings - 1) * boundary;
        let mut cursor = start;
        // edge numbers follow the DirectX 8 control-net diagram, independent of winding.
        let order = if key.triangular {
            [2usize, 1, 0, 0]
        } else {
            [0usize, 1, 2, 3]
        };
        for (edge, &factor_index) in order[..edges].iter().enumerate() {
            let n = counts[factor_index];
            for i in 0..n {
                let t = partition::knot(i, n, factors[factor_index], key.style);
                let inverse = partition::knot(n - i, n, factors[factor_index], key.style);
                self.vertices[cursor] = if key.triangular {
                    match edge {
                        0 => [t, 0.0],
                        1 => [inverse, t],
                        _ => [0.0, inverse],
                    }
                } else {
                    match edge {
                        0 => [t, 0.0],
                        1 => [1.0, t],
                        2 => [inverse, 1.0],
                        _ => [0.0, inverse],
                    }
                };
                cursor += 1;
            }
        }
        for ring in 1..rings {
            let radius = match key.style {
                EdgeStyle::Discrete => ring as f32 / rings as f32,
                EdgeStyle::Continuous => 2.0 * ring as f32 / max,
            };
            for i in 0..boundary {
                let outer = self.vertices[start + i];
                self.vertices[1 + (ring - 1) * boundary + i] = [
                    center[0] + (outer[0] - center[0]) * radius,
                    center[1] + (outer[1] - center[1]) * radius,
                ];
            }
        }
        for i in 0..boundary {
            let next = (i + 1) % boundary;
            self.indices
                .extend_from_slice(&[0, 1 + i as u32, 1 + next as u32]);
        }
        for ring in 1..rings {
            let inner = 1 + (ring - 1) * boundary;
            let outer = inner + boundary;
            for i in 0..boundary {
                let next = (i + 1) % boundary;
                self.indices.extend_from_slice(&[
                    inner as u32 + i as u32,
                    outer as u32 + i as u32,
                    outer as u32 + next as u32,
                    inner as u32 + i as u32,
                    outer as u32 + next as u32,
                    inner as u32 + next as u32,
                ]);
            }
        }
        self.boundary = boundary;
        self.rings = rings;
        self.key = Some(key);
        Ok(())
    }
}
