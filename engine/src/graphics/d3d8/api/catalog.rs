use super::InterfaceKind;

pub const CREATE_GATE: u32 = 0x0005_0001;
const METHOD_BASE: u32 = 0x0004_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Method {
    pub name: &'static str,
    pub stack_words: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodGate {
    pub interface: InterfaceKind,
    pub slot: u8,
}

impl MethodGate {
    pub const fn id(self) -> u32 {
        METHOD_BASE | ((self.interface as u32) << 8) | self.slot as u32
    }
    pub fn from_id(id: u32) -> Option<Self> {
        if id & 0xffff_0000 != METHOD_BASE {
            return None;
        }
        let interface = InterfaceKind::from_tag((id >> 8) as u8)?;
        let slot = id as u8;
        interface.methods().get(usize::from(slot))?;
        Some(Self { interface, slot })
    }
    pub fn method(self) -> &'static Method {
        &self.interface.methods()[usize::from(self.slot)]
    }
}

impl InterfaceKind {
    pub fn methods(self) -> &'static [Method] {
        match self {
            Self::Direct3D => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "RegisterSoftwareDevice",
                    stack_words: 2,
                },
                Method {
                    name: "GetAdapterCount",
                    stack_words: 1,
                },
                Method {
                    name: "GetAdapterIdentifier",
                    stack_words: 4,
                },
                Method {
                    name: "GetAdapterModeCount",
                    stack_words: 2,
                },
                Method {
                    name: "EnumAdapterModes",
                    stack_words: 4,
                },
                Method {
                    name: "GetAdapterDisplayMode",
                    stack_words: 3,
                },
                Method {
                    name: "CheckDeviceType",
                    stack_words: 6,
                },
                Method {
                    name: "CheckDeviceFormat",
                    stack_words: 7,
                },
                Method {
                    name: "CheckDeviceMultiSampleType",
                    stack_words: 6,
                },
                Method {
                    name: "CheckDepthStencilMatch",
                    stack_words: 6,
                },
                Method {
                    name: "GetDeviceCaps",
                    stack_words: 4,
                },
                Method {
                    name: "GetAdapterMonitor",
                    stack_words: 2,
                },
                Method {
                    name: "CreateDevice",
                    stack_words: 7,
                },
            ],
            Self::Device => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "TestCooperativeLevel",
                    stack_words: 1,
                },
                Method {
                    name: "GetAvailableTextureMem",
                    stack_words: 1,
                },
                Method {
                    name: "ResourceManagerDiscardBytes",
                    stack_words: 2,
                },
                Method {
                    name: "GetDirect3D",
                    stack_words: 2,
                },
                Method {
                    name: "GetDeviceCaps",
                    stack_words: 2,
                },
                Method {
                    name: "GetDisplayMode",
                    stack_words: 2,
                },
                Method {
                    name: "GetCreationParameters",
                    stack_words: 2,
                },
                Method {
                    name: "SetCursorProperties",
                    stack_words: 4,
                },
                Method {
                    name: "SetCursorPosition",
                    stack_words: 4,
                },
                Method {
                    name: "ShowCursor",
                    stack_words: 2,
                },
                Method {
                    name: "CreateAdditionalSwapChain",
                    stack_words: 3,
                },
                Method {
                    name: "Reset",
                    stack_words: 2,
                },
                Method {
                    name: "Present",
                    stack_words: 5,
                },
                Method {
                    name: "GetBackBuffer",
                    stack_words: 4,
                },
                Method {
                    name: "GetRasterStatus",
                    stack_words: 2,
                },
                Method {
                    name: "SetGammaRamp",
                    stack_words: 3,
                },
                Method {
                    name: "GetGammaRamp",
                    stack_words: 2,
                },
                Method {
                    name: "CreateTexture",
                    stack_words: 8,
                },
                Method {
                    name: "CreateVolumeTexture",
                    stack_words: 9,
                },
                Method {
                    name: "CreateCubeTexture",
                    stack_words: 7,
                },
                Method {
                    name: "CreateVertexBuffer",
                    stack_words: 6,
                },
                Method {
                    name: "CreateIndexBuffer",
                    stack_words: 6,
                },
                Method {
                    name: "CreateRenderTarget",
                    stack_words: 7,
                },
                Method {
                    name: "CreateDepthStencilSurface",
                    stack_words: 6,
                },
                Method {
                    name: "CreateImageSurface",
                    stack_words: 5,
                },
                Method {
                    name: "CopyRects",
                    stack_words: 6,
                },
                Method {
                    name: "UpdateTexture",
                    stack_words: 3,
                },
                Method {
                    name: "GetFrontBuffer",
                    stack_words: 2,
                },
                Method {
                    name: "SetRenderTarget",
                    stack_words: 3,
                },
                Method {
                    name: "GetRenderTarget",
                    stack_words: 2,
                },
                Method {
                    name: "GetDepthStencilSurface",
                    stack_words: 2,
                },
                Method {
                    name: "BeginScene",
                    stack_words: 1,
                },
                Method {
                    name: "EndScene",
                    stack_words: 1,
                },
                Method {
                    name: "Clear",
                    stack_words: 7,
                },
                Method {
                    name: "SetTransform",
                    stack_words: 3,
                },
                Method {
                    name: "GetTransform",
                    stack_words: 3,
                },
                Method {
                    name: "MultiplyTransform",
                    stack_words: 3,
                },
                Method {
                    name: "SetViewport",
                    stack_words: 2,
                },
                Method {
                    name: "GetViewport",
                    stack_words: 2,
                },
                Method {
                    name: "SetMaterial",
                    stack_words: 2,
                },
                Method {
                    name: "GetMaterial",
                    stack_words: 2,
                },
                Method {
                    name: "SetLight",
                    stack_words: 3,
                },
                Method {
                    name: "GetLight",
                    stack_words: 3,
                },
                Method {
                    name: "LightEnable",
                    stack_words: 3,
                },
                Method {
                    name: "GetLightEnable",
                    stack_words: 3,
                },
                Method {
                    name: "SetClipPlane",
                    stack_words: 3,
                },
                Method {
                    name: "GetClipPlane",
                    stack_words: 3,
                },
                Method {
                    name: "SetRenderState",
                    stack_words: 3,
                },
                Method {
                    name: "GetRenderState",
                    stack_words: 3,
                },
                Method {
                    name: "BeginStateBlock",
                    stack_words: 1,
                },
                Method {
                    name: "EndStateBlock",
                    stack_words: 2,
                },
                Method {
                    name: "ApplyStateBlock",
                    stack_words: 2,
                },
                Method {
                    name: "CaptureStateBlock",
                    stack_words: 2,
                },
                Method {
                    name: "DeleteStateBlock",
                    stack_words: 2,
                },
                Method {
                    name: "CreateStateBlock",
                    stack_words: 3,
                },
                Method {
                    name: "SetClipStatus",
                    stack_words: 2,
                },
                Method {
                    name: "GetClipStatus",
                    stack_words: 2,
                },
                Method {
                    name: "GetTexture",
                    stack_words: 3,
                },
                Method {
                    name: "SetTexture",
                    stack_words: 3,
                },
                Method {
                    name: "GetTextureStageState",
                    stack_words: 4,
                },
                Method {
                    name: "SetTextureStageState",
                    stack_words: 4,
                },
                Method {
                    name: "ValidateDevice",
                    stack_words: 2,
                },
                Method {
                    name: "GetInfo",
                    stack_words: 4,
                },
                Method {
                    name: "SetPaletteEntries",
                    stack_words: 3,
                },
                Method {
                    name: "GetPaletteEntries",
                    stack_words: 3,
                },
                Method {
                    name: "SetCurrentTexturePalette",
                    stack_words: 2,
                },
                Method {
                    name: "GetCurrentTexturePalette",
                    stack_words: 2,
                },
                Method {
                    name: "DrawPrimitive",
                    stack_words: 4,
                },
                Method {
                    name: "DrawIndexedPrimitive",
                    stack_words: 6,
                },
                Method {
                    name: "DrawPrimitiveUP",
                    stack_words: 5,
                },
                Method {
                    name: "DrawIndexedPrimitiveUP",
                    stack_words: 9,
                },
                Method {
                    name: "ProcessVertices",
                    stack_words: 6,
                },
                Method {
                    name: "CreateVertexShader",
                    stack_words: 5,
                },
                Method {
                    name: "SetVertexShader",
                    stack_words: 2,
                },
                Method {
                    name: "GetVertexShader",
                    stack_words: 2,
                },
                Method {
                    name: "DeleteVertexShader",
                    stack_words: 2,
                },
                Method {
                    name: "SetVertexShaderConstant",
                    stack_words: 4,
                },
                Method {
                    name: "GetVertexShaderConstant",
                    stack_words: 4,
                },
                Method {
                    name: "GetVertexShaderDeclaration",
                    stack_words: 4,
                },
                Method {
                    name: "GetVertexShaderFunction",
                    stack_words: 4,
                },
                Method {
                    name: "SetStreamSource",
                    stack_words: 4,
                },
                Method {
                    name: "GetStreamSource",
                    stack_words: 4,
                },
                Method {
                    name: "SetIndices",
                    stack_words: 3,
                },
                Method {
                    name: "GetIndices",
                    stack_words: 3,
                },
                Method {
                    name: "CreatePixelShader",
                    stack_words: 3,
                },
                Method {
                    name: "SetPixelShader",
                    stack_words: 2,
                },
                Method {
                    name: "GetPixelShader",
                    stack_words: 2,
                },
                Method {
                    name: "DeletePixelShader",
                    stack_words: 2,
                },
                Method {
                    name: "SetPixelShaderConstant",
                    stack_words: 4,
                },
                Method {
                    name: "GetPixelShaderConstant",
                    stack_words: 4,
                },
                Method {
                    name: "GetPixelShaderFunction",
                    stack_words: 4,
                },
                Method {
                    name: "DrawRectPatch",
                    stack_words: 4,
                },
                Method {
                    name: "DrawTriPatch",
                    stack_words: 4,
                },
                Method {
                    name: "DeletePatch",
                    stack_words: 2,
                },
            ],
            Self::Resource => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
            ],
            Self::BaseTexture => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
                Method {
                    name: "SetLOD",
                    stack_words: 2,
                },
                Method {
                    name: "GetLOD",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelCount",
                    stack_words: 1,
                },
            ],
            Self::Texture => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
                Method {
                    name: "SetLOD",
                    stack_words: 2,
                },
                Method {
                    name: "GetLOD",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelCount",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelDesc",
                    stack_words: 3,
                },
                Method {
                    name: "GetSurfaceLevel",
                    stack_words: 3,
                },
                Method {
                    name: "LockRect",
                    stack_words: 5,
                },
                Method {
                    name: "UnlockRect",
                    stack_words: 2,
                },
                Method {
                    name: "AddDirtyRect",
                    stack_words: 2,
                },
            ],
            Self::CubeTexture => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
                Method {
                    name: "SetLOD",
                    stack_words: 2,
                },
                Method {
                    name: "GetLOD",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelCount",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelDesc",
                    stack_words: 3,
                },
                Method {
                    name: "GetCubeMapSurface",
                    stack_words: 4,
                },
                Method {
                    name: "LockRect",
                    stack_words: 6,
                },
                Method {
                    name: "UnlockRect",
                    stack_words: 3,
                },
                Method {
                    name: "AddDirtyRect",
                    stack_words: 3,
                },
            ],
            Self::VolumeTexture => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
                Method {
                    name: "SetLOD",
                    stack_words: 2,
                },
                Method {
                    name: "GetLOD",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelCount",
                    stack_words: 1,
                },
                Method {
                    name: "GetLevelDesc",
                    stack_words: 3,
                },
                Method {
                    name: "GetVolumeLevel",
                    stack_words: 3,
                },
                Method {
                    name: "LockBox",
                    stack_words: 5,
                },
                Method {
                    name: "UnlockBox",
                    stack_words: 2,
                },
                Method {
                    name: "AddDirtyBox",
                    stack_words: 2,
                },
            ],
            Self::VertexBuffer => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
                Method {
                    name: "Lock",
                    stack_words: 5,
                },
                Method {
                    name: "Unlock",
                    stack_words: 1,
                },
                Method {
                    name: "GetDesc",
                    stack_words: 2,
                },
            ],
            Self::IndexBuffer => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "SetPriority",
                    stack_words: 2,
                },
                Method {
                    name: "GetPriority",
                    stack_words: 1,
                },
                Method {
                    name: "PreLoad",
                    stack_words: 1,
                },
                Method {
                    name: "GetType",
                    stack_words: 1,
                },
                Method {
                    name: "Lock",
                    stack_words: 5,
                },
                Method {
                    name: "Unlock",
                    stack_words: 1,
                },
                Method {
                    name: "GetDesc",
                    stack_words: 2,
                },
            ],
            Self::Surface => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "GetContainer",
                    stack_words: 3,
                },
                Method {
                    name: "GetDesc",
                    stack_words: 2,
                },
                Method {
                    name: "LockRect",
                    stack_words: 4,
                },
                Method {
                    name: "UnlockRect",
                    stack_words: 1,
                },
            ],
            Self::Volume => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "GetDevice",
                    stack_words: 2,
                },
                Method {
                    name: "SetPrivateData",
                    stack_words: 5,
                },
                Method {
                    name: "GetPrivateData",
                    stack_words: 4,
                },
                Method {
                    name: "FreePrivateData",
                    stack_words: 2,
                },
                Method {
                    name: "GetContainer",
                    stack_words: 3,
                },
                Method {
                    name: "GetDesc",
                    stack_words: 2,
                },
                Method {
                    name: "LockBox",
                    stack_words: 4,
                },
                Method {
                    name: "UnlockBox",
                    stack_words: 1,
                },
            ],
            Self::SwapChain => &[
                Method {
                    name: "QueryInterface",
                    stack_words: 3,
                },
                Method {
                    name: "AddRef",
                    stack_words: 1,
                },
                Method {
                    name: "Release",
                    stack_words: 1,
                },
                Method {
                    name: "Present",
                    stack_words: 5,
                },
                Method {
                    name: "GetBackBuffer",
                    stack_words: 4,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests;
