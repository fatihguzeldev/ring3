#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscalationClass32 {
    Benign,
    Contributory,
    PageFault,
    DoubleFault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecondaryClass32 {
    Benign,
    Contributory,
    PageFault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscalationDecision32 {
    DeliverSecondary,
    DeliverDoubleFault,
    Shutdown,
}

/// intel table7-5; selected same-cpl delivery produces only contributory/page secondary faults.
pub const fn escalate_exception(
    first: EscalationClass32,
    second: SecondaryClass32,
) -> EscalationDecision32 {
    use EscalationClass32::*;
    use EscalationDecision32::*;
    match (first, second) {
        (Benign, _)
        | (_, SecondaryClass32::Benign)
        | (Contributory, SecondaryClass32::PageFault) => DeliverSecondary,
        (Contributory, SecondaryClass32::Contributory)
        | (PageFault, SecondaryClass32::Contributory | SecondaryClass32::PageFault) => {
            DeliverDoubleFault
        }
        (DoubleFault, SecondaryClass32::Contributory | SecondaryClass32::PageFault) => Shutdown,
    }
}

#[cfg(test)]
mod tests;
