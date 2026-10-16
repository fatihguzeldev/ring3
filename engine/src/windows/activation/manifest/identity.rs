use super::super::ActivationError32 as E;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssemblyIdentity32 {
    pub name: String,
    pub version: [u16; 4],
    pub architecture: String,
    pub language: String,
    pub public_key_token: Option<[u8; 8]>,
    pub architecture_specified: bool,
    pub language_specified: bool,
}
impl AssemblyIdentity32 {
    pub fn satisfies(&self, reference: &Self) -> bool {
        self.name.eq_ignore_ascii_case(&reference.name)
            && self.version == reference.version
            && (reference.architecture == "*"
                || self
                    .architecture
                    .eq_ignore_ascii_case(&reference.architecture))
            && (reference.language == "*"
                || self.language.eq_ignore_ascii_case(&reference.language))
            && self.public_key_token == reference.public_key_token
    }
    pub(super) fn parse(node: roxmltree::Node<'_, '_>) -> Result<Self, E> {
        Self::parse_kind(node, "win32", true)
    }
    pub(in crate::windows::activation) fn parse_reference(
        node: roxmltree::Node<'_, '_>,
    ) -> Result<Self, E> {
        Self::parse_kind(node, "win32", false)
    }
    pub(in crate::windows::activation) fn parse_policy(
        node: roxmltree::Node<'_, '_>,
    ) -> Result<Self, E> {
        Self::parse_kind(node, "win32-policy", true)
    }
    fn parse_kind(
        node: roxmltree::Node<'_, '_>,
        kind: &str,
        required_version: bool,
    ) -> Result<Self, E> {
        if node.attribute("type").is_some_and(|value| value != kind) {
            return Err(E::ManifestFormat);
        }
        let name = node.attribute("name").ok_or(E::ManifestFormat)?;
        if name.is_empty()
            || matches!(name, "." | "..")
            || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
            || name.chars().any(char::is_control)
        {
            return Err(E::ManifestFormat);
        }
        let version = match node.attribute("version") {
            Some(value) => version(value)?,
            None if !required_version => [0; 4],
            None => return Err(E::ManifestFormat),
        };
        let architecture = node.attribute("processorArchitecture").unwrap_or("x86");
        if !architecture.eq_ignore_ascii_case("x86") && architecture != "*" {
            return Err(E::IdentityMismatch);
        }
        let language = node.attribute("language").unwrap_or("*");
        if language != "*"
            && (language.is_empty()
                || !language
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
        {
            return Err(E::ManifestFormat);
        }
        let public_key_token = node
            .attribute("publicKeyToken")
            .map(hex_bytes)
            .transpose()?;
        Ok(Self {
            name: name.into(),
            version,
            architecture: architecture.into(),
            language: language.into(),
            public_key_token,
            architecture_specified: node.attribute("processorArchitecture").is_some(),
            language_specified: node.attribute("language").is_some(),
        })
    }
}
pub(in crate::windows::activation) fn version(value: &str) -> Result<[u16; 4], E> {
    let mut result = [0; 4];
    let mut parts = value.split('.');
    for part in &mut result {
        let text = parts.next().ok_or(E::ManifestFormat)?;
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(E::ManifestFormat);
        }
        *part = text.parse().map_err(|_| E::ManifestFormat)?;
    }
    if parts.next().is_some() {
        return Err(E::ManifestFormat);
    }
    Ok(result)
}
pub(super) fn hex_bytes<const N: usize>(value: &str) -> Result<[u8; N], E> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(E::ManifestFormat);
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| E::ManifestFormat)?;
    }
    Ok(bytes)
}
pub(super) fn guid(value: &str) -> Result<[u8; 16], E> {
    let value = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(value);
    if value.len() != 36
        || ![8, 13, 18, 23]
            .iter()
            .all(|&index| value.as_bytes()[index] == b'-')
    {
        return Err(E::ManifestFormat);
    }
    let compact = value.replace('-', "");
    let mut bytes = hex_bytes::<16>(&compact)?;
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Ok(bytes)
}
