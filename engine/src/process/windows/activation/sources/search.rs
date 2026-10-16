use crate::{
    files::FileError32,
    process::{EngineInstance, HostError},
    windows::{
        activation::{ActivationError32 as E, manifest::AssemblyIdentity32},
        nls::locales::locale_identity,
    },
};

pub(super) struct ManifestCandidate32 {
    pub(super) path: Vec<u16>,
    pub(super) directory: Option<Vec<u16>>,
    pub(super) language: Option<String>,
}
pub(super) fn absolute(path: &[u16]) -> bool {
    path.get(1) == Some(&u16::from(b':')) || path.starts_with(&[u16::from(b'\\')])
}
pub(in crate::process::windows::activation) fn directory(path: &[u16]) -> Vec<u16> {
    path.iter()
        .rposition(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))
        .map_or_else(Vec::new, |end| path[..end + 1].to_vec())
}
pub(super) fn join(directory: &[u16], path: &[u16]) -> Vec<u16> {
    let mut result = directory.to_vec();
    if !result.ends_with(&[u16::from(b'\\')]) {
        result.push(u16::from(b'\\'));
    }
    result.extend_from_slice(path);
    result
}
pub(super) fn candidates(
    engine: &EngineInstance,
    directory: &[u16],
    identity: &AssemblyIdentity32,
    language: u16,
    limit: usize,
) -> Result<Vec<ManifestCandidate32>, HostError> {
    let mut languages = Vec::new();
    let specific = if identity.language != "*" {
        identity.language.clone()
    } else {
        locale_identity(u32::from(language))
            .map(|locale| locale.language_tag().to_owned())
            .unwrap_or_default()
    };
    if !specific.is_empty() {
        languages.push(specific.clone());
        if let Some((parent, _)) = specific.split_once('-') {
            languages.push(parent.into());
        }
    }
    for fallback in ["en-US", "en", ""] {
        if !languages.iter().any(|language| language == fallback) {
            languages.push(fallback.into());
        }
    }
    let mut paths = Vec::new();
    for language in languages {
        if let Some(files) = engine.files.as_ref() {
            let windows = files.virtual_windows_directory();
            let token = identity.public_key_token.map_or_else(
                || "none".into(),
                |token| {
                    token
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                },
            );
            let version = identity.version;
            let pattern = format!(
                "WinSxS\\Manifests\\x86_{}_{}_{}.{}.{}.{}_{}_*.manifest",
                identity.name,
                token,
                version[0],
                version[1],
                version[2],
                version[3],
                if language.is_empty() {
                    "none"
                } else {
                    &language
                }
            );
            match engine.find_provider_paths(
                &join(windows, &pattern.encode_utf16().collect::<Vec<_>>()),
                limit,
            ) {
                Ok(shared) => {
                    for path in shared {
                        let stem = path
                            .iter()
                            .rposition(|&unit| unit == u16::from(b'\\'))
                            .map_or(0, |index| index + 1);
                        let filename = &path[stem..];
                        let suffix = ".manifest".encode_utf16().count();
                        let stem = filename
                            .get(..filename.len().saturating_sub(suffix))
                            .ok_or(HostError::Activation(E::ManifestFormat))?;
                        let directory = join(
                            &join(windows, &"WinSxS".encode_utf16().collect::<Vec<_>>()),
                            stem,
                        );
                        paths.push(ManifestCandidate32 {
                            path,
                            directory: Some(directory),
                            language: Some(language.clone()),
                        });
                    }
                }
                Err(HostError::File(
                    FileError32::NotFound | FileError32::PathNotFound | FileError32::NoMoreFiles,
                )) => {}
                Err(error) => return Err(error),
            }
        }
        let base = if language.is_empty() {
            directory.to_vec()
        } else {
            join(directory, &language.encode_utf16().collect::<Vec<_>>())
        };
        for child in [false, true] {
            let base = if child {
                join(&base, &identity.name.encode_utf16().collect::<Vec<_>>())
            } else {
                base.clone()
            };
            for extension in ["dll", "manifest"] {
                paths.push(ManifestCandidate32 {
                    path: join(
                        &base,
                        &format!("{}.{}", identity.name, extension)
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    ),
                    directory: None,
                    language: Some(language.clone()),
                });
            }
        }
    }
    let cost = paths
        .iter()
        .map(|candidate| {
            (candidate.path.len() + candidate.directory.as_ref().map_or(0, Vec::len)) * 2
        })
        .sum::<usize>();
    if cost > limit.saturating_mul(65536) {
        return Err(HostError::Activation(E::NotEnoughMemory));
    }
    Ok(paths)
}

pub(super) fn publisher_candidates(
    engine: &EngineInstance,
    identity: &AssemblyIdentity32,
    limit: usize,
) -> Result<Vec<ManifestCandidate32>, HostError> {
    let Some(files) = &engine.files else {
        return Ok(Vec::new());
    };
    let Some(token) = identity.public_key_token else {
        return Ok(Vec::new());
    };
    let token = token
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let name = format!(
        "policy.{}.{}.{}",
        identity.version[0], identity.version[1], identity.name
    );
    let windows = files.virtual_windows_directory();
    let mut paths = optional_paths(
        engine,
        &join(
            windows,
            &format!("WinSxS\\Manifests\\x86_{name}_{token}_*.manifest")
                .encode_utf16()
                .collect::<Vec<_>>(),
        ),
        limit,
    )?;
    let directories = optional_paths(
        engine,
        &join(
            windows,
            &format!("WinSxS\\Policies\\x86_{name}_{token}_*")
                .encode_utf16()
                .collect::<Vec<_>>(),
        ),
        limit,
    )?;
    for directory in directories {
        paths.extend(optional_paths(
            engine,
            &join(&directory, &"*.policy".encode_utf16().collect::<Vec<_>>()),
            limit,
        )?);
        if paths.len() > limit {
            return Err(HostError::Activation(E::NotEnoughMemory));
        }
    }
    Ok(paths
        .into_iter()
        .map(|path| ManifestCandidate32 {
            path,
            directory: None,
            language: None,
        })
        .collect())
}
fn optional_paths(
    engine: &EngineInstance,
    pattern: &[u16],
    limit: usize,
) -> Result<Vec<Vec<u16>>, HostError> {
    match engine.find_provider_paths(pattern, limit) {
        Ok(paths) => Ok(paths),
        Err(HostError::File(
            FileError32::NotFound | FileError32::PathNotFound | FileError32::NoMoreFiles,
        )) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}
