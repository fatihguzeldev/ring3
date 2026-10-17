use super::*;

#[test]
fn parts_prepare_keeps_old_payloads_and_simple_text_separate_until_commit() {
    let mut status = Status32::default();
    status.part_mut(0).unwrap().text = vec![65];
    status.text_part_mut(0xff).unwrap().text = vec![66];
    let prepared = Status32::prepare_parts(vec![20, 40, -1]).unwrap();
    assert_eq!(status.count(), 1);
    assert_eq!(status.part(0).unwrap().right, -1);
    status.commit_parts(prepared);
    assert_eq!(status.count(), 3);
    assert_eq!(status.part(0).unwrap().text, [65]);
    assert_eq!(status.text_part(0xff).unwrap().text, [66]);
    assert_eq!(
        status.rect(1, 100, 20),
        Some(Rect32 {
            left: 20,
            top: 2,
            right: 40,
            bottom: 20
        })
    );
    assert_eq!(status.rect(2, 100, 20).unwrap().right, 100);
    assert_eq!(status.rect(3, 100, 20), None);
}

#[test]
fn part_and_text_budgets_refuse_before_old_values_change() {
    let mut status = Status32::default();
    status.part_mut(0).unwrap().text = vec![65];
    assert!(Status32::prepare_parts(vec![0; MAX_PARTS + 1]).is_err());
    assert!(status.check_text(0, MAX_TEXT + 1).is_err());
    assert_eq!(status.part(0).unwrap().text, [65]);
    assert_eq!(status.count(), 1);
}
