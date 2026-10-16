use super::{
    AssemblyDependency32, AssemblyFile32, AssemblyIdentity32, ComClass32, ComInterface32,
    TypeLibrary32, VersionRedirect32, WindowClass32,
    identity::{guid, version},
    parser::ASSEMBLY,
};
use crate::windows::{activation::ActivationError32 as E, com::ServerThreadingModel32};

pub(super) fn dependency(node: roxmltree::Node<'_, '_>) -> Result<AssemblyDependency32, E> {
    let optional = boolean(node.attribute("optional").unwrap_or("no"))?;
    let mut children = node.children().filter(|node| node.is_element());
    let assembly = children.next().ok_or(E::ManifestFormat)?;
    if !assembly.has_tag_name((ASSEMBLY, "dependentAssembly")) || children.next().is_some() {
        return Err(E::ManifestFormat);
    }
    let mut children = assembly.children().filter(|node| node.is_element());
    let identity = children.next().ok_or(E::ManifestFormat)?;
    if !identity.has_tag_name((ASSEMBLY, "assemblyIdentity")) {
        return Err(E::ManifestFormat);
    }
    let mut result = AssemblyDependency32 {
        identity: AssemblyIdentity32::parse(identity)?,
        optional,
        redirects: Vec::new(),
    };
    for node in children {
        if !node.has_tag_name((ASSEMBLY, "bindingRedirect")) {
            return Err(E::ManifestFormat);
        }
        let original = node.attribute("oldVersion").ok_or(E::ManifestFormat)?;
        let (minimum, maximum) = if let Some((minimum, maximum)) = original.split_once('-') {
            (version(minimum)?, version(maximum)?)
        } else {
            let value = version(original)?;
            (value, value)
        };
        if minimum > maximum {
            return Err(E::ManifestFormat);
        }
        result.redirects.push(VersionRedirect32 {
            minimum,
            maximum,
            target: version(node.attribute("newVersion").ok_or(E::ManifestFormat)?)?,
        });
    }
    Ok(result)
}

pub(super) fn file(node: roxmltree::Node<'_, '_>) -> Result<AssemblyFile32, E> {
    let name = node.attribute("name").ok_or(E::ManifestFormat)?;
    if name.is_empty()
        || name.contains([':', '\0'])
        || name.starts_with(['/', '\\'])
        || name
            .split(['/', '\\'])
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(E::ManifestFormat);
    }
    let mut result = AssemblyFile32 {
        name: name.into(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_libraries: Vec::new(),
        window_classes: Vec::new(),
    };
    for node in node.children().filter(|node| node.is_element()) {
        if node.tag_name().namespace() != Some(ASSEMBLY) {
            continue;
        }
        match node.tag_name().name() {
            "comClass" => result.classes.push(class(node)?),
            "comInterfaceProxyStub" => result.interfaces.push(interface(node)?),
            "typelib" => result.type_libraries.push(type_library(node)?),
            "windowClass" => {
                let name = node.text().ok_or(E::ManifestFormat)?.trim();
                if name.is_empty() {
                    return Err(E::ManifestFormat);
                }
                result.window_classes.push(WindowClass32 {
                    name: name.into(),
                    versioned: boolean(node.attribute("versioned").unwrap_or("yes"))?,
                });
            }
            _ => {}
        }
    }
    Ok(result)
}

fn class(node: roxmltree::Node<'_, '_>) -> Result<ComClass32, E> {
    let mut result = ComClass32 {
        clsid: guid(node.attribute("clsid").ok_or(E::ManifestFormat)?)?,
        threading: threading(node)?,
        typelib: node.attribute("tlbid").map(guid).transpose()?,
        progids: node
            .attribute("progid")
            .map(str::to_owned)
            .into_iter()
            .collect(),
        description: node.attribute("description").map(str::to_owned),
        misc_status: [0; 5],
        misc_mask: 0,
    };
    for (index, name) in [
        "miscStatus",
        "miscStatusContent",
        "miscStatusThumbnail",
        "miscStatusIcon",
        "miscStatusDocPrint",
    ]
    .into_iter()
    .enumerate()
    {
        if node.attribute(name).is_some() {
            result.misc_mask |= [1, 4, 8, 2, 16][index];
        }
        result.misc_status[index] = node
            .attribute(name)
            .map(misc_status)
            .transpose()?
            .unwrap_or(0);
    }
    for child in node.children().filter(|node| node.is_element()) {
        if child.has_tag_name((ASSEMBLY, "progid")) {
            let value = child.text().ok_or(E::ManifestFormat)?.trim();
            if value.is_empty() {
                return Err(E::ManifestFormat);
            }
            result.progids.push(value.into());
        }
    }
    Ok(result)
}

pub(super) fn interface(node: roxmltree::Node<'_, '_>) -> Result<ComInterface32, E> {
    Ok(ComInterface32 {
        iid: guid(node.attribute("iid").ok_or(E::ManifestFormat)?)?,
        name: node.attribute("name").map(str::to_owned),
        proxy: node.attribute("proxyStubClsid32").map(guid).transpose()?,
        typelib: node.attribute("tlbid").map(guid).transpose()?,
        base: node.attribute("baseInterface").map(guid).transpose()?,
        methods: node.attribute("numMethods").map(number).transpose()?,
        threading: threading(node)?,
    })
}

fn type_library(node: roxmltree::Node<'_, '_>) -> Result<TypeLibrary32, E> {
    let version = node.attribute("version").ok_or(E::ManifestFormat)?;
    let (major, minor) = version.split_once('.').ok_or(E::ManifestFormat)?;
    let flags = node
        .attribute("flags")
        .unwrap_or("")
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .try_fold(0, |flags, name| {
            Ok(flags
                | match name.trim().to_ascii_uppercase().as_str() {
                    "RESTRICTED" => 1,
                    "CONTROL" => 2,
                    "HIDDEN" => 4,
                    "HASDISKIMAGE" => 8,
                    _ => return Err(E::ManifestFormat),
                })
        })?;
    Ok(TypeLibrary32 {
        id: guid(node.attribute("tlbid").ok_or(E::ManifestFormat)?)?,
        version: [
            number(major)?.try_into().map_err(|_| E::ManifestFormat)?,
            number(minor)?.try_into().map_err(|_| E::ManifestFormat)?,
        ],
        resource: number(node.attribute("resourceid").unwrap_or("0"))?
            .try_into()
            .map_err(|_| E::ManifestFormat)?,
        flags,
        help_directory: node.attribute("helpdir").unwrap_or("").into(),
    })
}

fn threading(node: roxmltree::Node<'_, '_>) -> Result<ServerThreadingModel32, E> {
    Ok(
        match node
            .attribute("threadingModel")
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "" => ServerThreadingModel32::MainSta,
            "apartment" => ServerThreadingModel32::Apartment,
            "free" => ServerThreadingModel32::Free,
            "both" => ServerThreadingModel32::Both,
            "neutral" => ServerThreadingModel32::Neutral,
            _ => return Err(E::ManifestFormat),
        },
    )
}
fn boolean(value: &str) -> Result<bool, E> {
    match value.to_ascii_lowercase().as_str() {
        "yes" | "true" => Ok(true),
        "no" | "false" => Ok(false),
        _ => Err(E::ManifestFormat),
    }
}
fn number(value: &str) -> Result<u32, E> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).map_err(|_| E::ManifestFormat)
    } else {
        value.parse().map_err(|_| E::ManifestFormat)
    }
}
fn misc_status(value: &str) -> Result<u32, E> {
    if let Ok(number) = number(value) {
        return Ok(number);
    }
    value.split(',').try_fold(0, |flags, name| {
        Ok(flags
            | match name.trim().to_ascii_lowercase().as_str() {
                "recomposeonresize" => 1,
                "onlyiconic" => 2,
                "insertnotreplace" => 4,
                "static" => 8,
                "cantlinkinside" => 16,
                "canlinkbyole1" => 32,
                "islinkobject" => 64,
                "insideout" => 128,
                "activatewhenvisible" => 256,
                "renderingisdeviceindependent" => 512,
                "invisibleatruntime" => 1024,
                "alwaysrun" => 2048,
                "actslikebutton" => 4096,
                "actslikelabel" => 8192,
                "nouiactivate" => 16384,
                "alignable" => 32768,
                "simpleframe" => 65536,
                "setclientsitefirst" => 131072,
                "imemode" => 262144,
                "ignoreactivatewhenvisible" => 524288,
                "wantstomenumerge" => 1048576,
                "supportsmultilevelundo" => 2097152,
                _ => return Err(E::ManifestFormat),
            })
    })
}
