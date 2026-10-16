use super::{BindingPolicy32, BindingRedirect32};
use crate::windows::activation::{
    ActivationConfig32, ActivationError32 as E,
    manifest::{self, AssemblyIdentity32},
};
const ASSEMBLY: &str = "urn:schemas-microsoft-com:asm.v1";

impl BindingPolicy32 {
    pub fn application(bytes: &[u8], limits: ActivationConfig32) -> Result<Self, E> {
        parse(bytes, limits, false)
    }
    pub fn publisher(bytes: &[u8], limits: ActivationConfig32) -> Result<Self, E> {
        parse(bytes, limits, true)
    }
}
fn parse(bytes: &[u8], limits: ActivationConfig32, publisher: bool) -> Result<BindingPolicy32, E> {
    if bytes.len() > limits.manifest_bytes as usize {
        return Err(E::NotEnoughMemory);
    }
    let text = manifest::decode(bytes)?;
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
    let mut result = BindingPolicy32::default();
    let binding = if publisher {
        if !root.has_tag_name((ASSEMBLY, "assembly"))
            || root.attribute("manifestVersion") != Some("1.0")
        {
            return Err(E::ManifestFormat);
        }
        root
    } else {
        if root.tag_name().name() != "configuration" {
            return Err(E::ManifestFormat);
        }
        let Some(windows) = root
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "windows")
        else {
            return Ok(result);
        };
        for policy in windows
            .children()
            .filter(|node| node.is_element() && node.tag_name().name() == "publisherPolicy")
        {
            result.publisher = match policy
                .attribute("apply")
                .unwrap_or("yes")
                .to_ascii_lowercase()
                .as_str()
            {
                "yes" => true,
                "no" => false,
                _ => return Err(E::ManifestFormat),
            };
        }
        let Some(binding) = windows
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "assemblyBinding")
        else {
            return Ok(result);
        };
        binding
    };
    let mut children = binding.children().filter(|node| node.is_element());
    let identity = children.next().ok_or(E::ManifestFormat)?;
    if identity.tag_name().name() != "assemblyIdentity" {
        return Err(E::ManifestFormat);
    }
    result.identity = Some(if publisher {
        AssemblyIdentity32::parse_policy(identity)?
    } else {
        AssemblyIdentity32::parse_reference(identity)?
    });
    for child in children {
        match child.tag_name().name() {
            "dependency" => {
                for dependency in child.children().filter(|node| node.is_element()) {
                    if dependency.tag_name().name() != "dependentAssembly" {
                        return Err(E::ManifestFormat);
                    }
                    redirects(&mut result, dependency)?;
                }
            }
            "dependentAssembly" => redirects(&mut result, child)?,
            "probing" if !publisher => {
                if !result.private_paths.is_empty() {
                    return Err(E::ManifestFormat);
                }
                result.private_paths = super::paths::private_paths(
                    child.attribute("privatePath").ok_or(E::ManifestFormat)?,
                )?;
            }
            _ => return Err(E::ManifestFormat),
        }
    }
    Ok(result)
}
fn redirects(policy: &mut BindingPolicy32, node: roxmltree::Node<'_, '_>) -> Result<(), E> {
    let mut children = node.children().filter(|node| node.is_element());
    let identity = children.next().ok_or(E::ManifestFormat)?;
    if identity.tag_name().name() != "assemblyIdentity" {
        return Err(E::ManifestFormat);
    }
    let identity = AssemblyIdentity32::parse_reference(identity)?;
    let mut any = false;
    for child in children {
        if child.tag_name().name() != "bindingRedirect" {
            return Err(E::ManifestFormat);
        }
        let old = child.attribute("oldVersion").ok_or(E::ManifestFormat)?;
        let (minimum, maximum) = old.split_once('-').map_or((old, old), |range| range);
        let minimum = manifest::version(minimum)?;
        let maximum = manifest::version(maximum)?;
        let target = manifest::version(child.attribute("newVersion").ok_or(E::ManifestFormat)?)?;
        if minimum > maximum || minimum[..2] != target[..2] || maximum[..2] != target[..2] {
            return Err(E::ManifestFormat);
        }
        policy.redirects.push(BindingRedirect32 {
            identity: identity.clone(),
            minimum,
            maximum,
            target,
        });
        any = true;
    }
    if !any {
        return Err(E::ManifestFormat);
    }
    Ok(())
}
