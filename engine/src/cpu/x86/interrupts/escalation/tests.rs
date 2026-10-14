use super::*;

#[test]
fn every_intel_table7_5_cell_is_literal_and_automatic_escalation_is_not_implied() {
    use EscalationClass32::*;
    use EscalationDecision32::*;
    for (first, outcomes) in [
        (
            Benign,
            [DeliverSecondary, DeliverSecondary, DeliverSecondary],
        ),
        (
            Contributory,
            [DeliverSecondary, DeliverDoubleFault, DeliverSecondary],
        ),
        (
            PageFault,
            [DeliverSecondary, DeliverDoubleFault, DeliverDoubleFault],
        ),
        (DoubleFault, [DeliverSecondary, Shutdown, Shutdown]),
    ] {
        for (second, expected) in [
            SecondaryClass32::Benign,
            SecondaryClass32::Contributory,
            SecondaryClass32::PageFault,
        ]
        .into_iter()
        .zip(outcomes)
        {
            assert_eq!(escalate_exception(first, second), expected);
        }
    }
}
