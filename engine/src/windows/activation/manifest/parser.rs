use super::{AssemblyIdentity32, Manifest32, RequestedLevel32, identity::guid};
use crate::windows::activation::{ActivationConfig32, ActivationError32 as E};

pub(super) const ASSEMBLY: &str = "urn:schemas-microsoft-com:asm.v1";
pub(super) const ASSEMBLY_V3: &str = "urn:schemas-microsoft-com:asm.v3";

impl Manifest32 {
    pub fn parse(bytes: &[u8], limits: ActivationConfig32) -> Result<Self, E> {
        if bytes.len() > limits.manifest_bytes as usize {
            return Err(E::NotEnoughMemory);
        }
        let text = decode(bytes)?;
        let document = roxmltree::Document::parse_with_options(
            &text,
            roxmltree::ParsingOptions {
                allow_dtd: false,
                nodes_limit: limits.nodes,
                entity_resolver: None,
            },
        )
        .map_err(|_| E::ManifestParse)?;
        let root = document.root_element();
        if !root.has_tag_name((ASSEMBLY, "assembly"))
            || root.attribute("manifestVersion") != Some("1.0")
        {
            return Err(E::ManifestFormat);
        }
        let mut children = root.children().filter(|node| node.is_element());
        let first = children.next().ok_or(E::ManifestFormat)?;
        let no_inherit = first.has_tag_name((ASSEMBLY, "noInherit"));
        let no_inheritable = first.has_tag_name((ASSEMBLY, "noInheritable"));
        let identity = if no_inherit || no_inheritable {
            children.next().ok_or(E::ManifestFormat)?
        } else {
            first
        };
        if !identity.has_tag_name((ASSEMBLY, "assemblyIdentity")) {
            return Err(E::ManifestFormat);
        }
        let mut result = Self {
            identity: AssemblyIdentity32::parse(identity)?,
            no_inherit,
            no_inheritable,
            dependencies: Vec::new(),
            files: Vec::new(),
            interfaces: Vec::new(),
            requested_level: None,
            supported_os: Vec::new(),
            settings: Vec::new(),
        };
        for node in children {
            match (node.tag_name().namespace(), node.tag_name().name()) {
                (Some(ASSEMBLY), "assemblyIdentity" | "noInherit") => {
                    return Err(E::ManifestFormat);
                }
                (Some(ASSEMBLY), "noInheritable") => result.no_inheritable = true,
                (Some(ASSEMBLY), "dependency") => {
                    result.dependencies.push(super::records::dependency(node)?);
                }
                (Some(ASSEMBLY), "file") => result.files.push(super::records::file(node)?),
                (Some(ASSEMBLY), "comInterfaceExternalProxyStub") => {
                    result.interfaces.push(super::records::interface(node)?);
                }
                (Some(ASSEMBLY_V3), "trustInfo") | (Some(ASSEMBLY), "trustInfo") => {
                    result.requested_level = requested_level(node)?;
                }
                (Some(ASSEMBLY_V3), "application") => {
                    for settings in node
                        .children()
                        .filter(|node| node.has_tag_name((ASSEMBLY_V3, "windowsSettings")))
                    {
                        for value in settings.children().filter(|node| node.is_element()) {
                            let namespace =
                                value.tag_name().namespace().ok_or(E::ManifestFormat)?;
                            if namespace.starts_with("http://schemas.microsoft.com/SMI/") {
                                result.settings.push(super::ManifestSetting32 {
                                    namespace: namespace.into(),
                                    name: value.tag_name().name().into(),
                                    value: value.text().unwrap_or("").trim().into(),
                                });
                            }
                        }
                    }
                }
                (Some("urn:schemas-microsoft-com:compatibility.v1"), "compatibility") => {
                    for os in node.descendants().filter(|node| {
                        node.has_tag_name((
                            "urn:schemas-microsoft-com:compatibility.v1",
                            "supportedOS",
                        ))
                    }) {
                        result
                            .supported_os
                            .push(guid(os.attribute("Id").ok_or(E::ManifestFormat)?)?);
                    }
                }
                _ => {}
            }
        }
        super::validation::validate(&result)?;
        Ok(result)
    }
}

pub(in crate::windows::activation) fn decode(bytes: &[u8]) -> Result<String, E> {
    let (bytes, little) = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        (bytes, Some(true))
    } else if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        (bytes, Some(false))
    } else if bytes.starts_with(&[b'<', 0]) {
        (bytes, Some(true))
    } else if bytes.starts_with(&[0, b'<']) {
        (bytes, Some(false))
    } else {
        (bytes, None)
    };
    let text = if let Some(little) = little {
        if !bytes.len().is_multiple_of(2) {
            return Err(E::ManifestParse);
        }
        let units = bytes
            .chunks_exact(2)
            .map(|bytes| {
                if little {
                    u16::from_le_bytes([bytes[0], bytes[1]])
                } else {
                    u16::from_be_bytes([bytes[0], bytes[1]])
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|_| E::ManifestParse)?
    } else {
        core::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map_err(|_| E::ManifestParse)?
            .into()
    };
    Ok(text.trim_end_matches('\0').into())
}

fn requested_level(node: roxmltree::Node<'_, '_>) -> Result<Option<RequestedLevel32>, E> {
    let mut levels = node.descendants().filter(|node| {
        node.has_tag_name((ASSEMBLY_V3, "requestedExecutionLevel"))
            || node.has_tag_name((ASSEMBLY, "requestedExecutionLevel"))
    });
    let Some(level) = levels.next() else {
        return Ok(None);
    };
    if levels.next().is_some() {
        return Err(E::ManifestFormat);
    }
    let ui_access = match level.attribute("uiAccess").unwrap_or("false") {
        "true" => true,
        "false" => false,
        _ => return Err(E::ManifestFormat),
    };
    Ok(Some(match level.attribute("level") {
        Some("asInvoker") => RequestedLevel32::AsInvoker { ui_access },
        Some("highestAvailable") => RequestedLevel32::HighestAvailable { ui_access },
        Some("requireAdministrator") => RequestedLevel32::RequireAdministrator { ui_access },
        _ => return Err(E::ManifestFormat),
    }))
}
