use super::*;

#[test]
fn infinite_dividend_does_not_deliver_an_unmasked_zero_divide_exception() {
    for negative_numerator in [false, true] {
        for negative_denominator in [false, true] {
            let mut state = X87State {
                control: 0x037b,
                ..X87State::default()
            };
            let zero = Ext80::from_parts(negative_denominator, 0, 0);
            let infinity = Ext80::from_parts(negative_numerator, 0x7fff, 1 << 63);
            state = state
                .evaluate(X87Operation::Load(X87Format::Extended80), &zero.0, 0)
                .unwrap()
                .state;
            state = state
                .evaluate(X87Operation::Load(X87Format::Extended80), &infinity.0, 0)
                .unwrap()
                .state;
            let result = state
                .evaluate(
                    X87Operation::Binary {
                        kind: X87Binary::Divide,
                        destination: 0,
                        source: 1,
                        pop: false,
                    },
                    &[],
                    0,
                )
                .unwrap();
            assert!(!result.unmasked_exception);
            assert_eq!(result.state.status & 0x8084, 0);
            assert_eq!(
                result.state.stack(0),
                Some(Ext80::from_parts(
                    negative_numerator ^ negative_denominator,
                    0x7fff,
                    1 << 63
                ))
            );
            assert_eq!(result.state.stack(1), Some(zero));
            assert_eq!(result.state.top(), state.top());
        }
    }
}
