use super::{
    DeviceKind32, DirectInputResult32,
    format::{DeviceDescription32, DeviceObject32},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action32 {
    pub app_data: u32,
    pub semantic: u32,
    pub flags: u32,
    pub name: u32,
    pub instance: [u8; 16],
    pub object_id: u32,
    pub how: u32,
}
pub fn default_object(description: DeviceDescription32, semantic: u32) -> Option<DeviceObject32> {
    let source = semantic >> 24;
    let instance = semantic & 255;
    let object = match (description.kind, source, (semantic >> 8) & 7) {
        (DeviceKind32::Keyboard, 0x81, 4) => DeviceObject32::Key(instance as u16),
        (DeviceKind32::Mouse, 0x82, 2 | 3) => DeviceObject32::Axis {
            index: (instance / 4) as u8,
            aspect: 0,
        },
        (DeviceKind32::Mouse, 0x82, 4) if instance >= 12 => {
            DeviceObject32::Button((instance - 12) as u16)
        }
        (DeviceKind32::Controller(_), source, 2) if source != 0x81 && source != 0x82 => {
            let axis = ((semantic >> 15) & 31).saturating_sub(1);
            DeviceObject32::Axis {
                index: axis as u8,
                aspect: 0,
            }
        }
        (DeviceKind32::Controller(_), source, 4)
            if source != 0x81 && source != 0x82 && instance > 0 =>
        {
            DeviceObject32::Button((instance - 1) as u16)
        }
        (DeviceKind32::Controller(_), source, 6)
            if source != 0x81 && source != 0x82 && instance > 0 =>
        {
            DeviceObject32::Pov((instance - 1) as u8)
        }
        _ => return None,
    };
    description.objects().find(|candidate| *candidate == object)
}
pub fn build_actions(
    description: DeviceDescription32,
    instance: [u8; 16],
    actions: &mut [Action32],
    preserve: bool,
) -> Result<usize, DirectInputResult32> {
    build_actions_with_haptics(description, instance, actions, preserve, None)
}
pub fn build_actions_with_haptics(
    description: DeviceDescription32,
    instance: [u8; 16],
    actions: &mut [Action32],
    preserve: bool,
    haptics: Option<super::haptics::HapticCapabilities32>,
) -> Result<usize, DirectInputResult32> {
    if actions.iter().any(|action| action.flags & !31 != 0) {
        return Err(DirectInputResult32::INVALID_PARAM);
    }
    let mut mapped = 0;
    for action in actions {
        if action.flags & 4 != 0 || action.flags & 1 != 0 && haptics.is_none() {
            action.how = 0;
            continue;
        }
        if action.flags & 2 != 0 || preserve && action.instance != [0; 16] {
            if action.instance != instance {
                continue;
            }
            let object = description.objects().find(|object| {
                object.instance() == ((action.object_id >> 8) & 0xffff) as u16
                    && object.object_type(description.kind == DeviceKind32::Mouse)
                        & action.object_id
                        & 255
                        != 0
            });
            let valid =
                object.is_some_and(|object| action.flags & 1 == 0 || force_object(object, haptics));
            action.how = if valid { 2 } else { 0x8000_0000 };
            mapped += usize::from(valid);
            continue;
        }
        if let Some(object) = default_object(description, action.semantic)
            .filter(|object| action.flags & 1 == 0 || force_object(*object, haptics))
        {
            action.instance = instance;
            action.object_id = object.object_type(matches!(description.kind, DeviceKind32::Mouse));
            action.how = 0x20;
            mapped += 1;
        } else {
            action.instance = [0; 16];
            action.object_id = 0;
            action.how = 0;
        }
    }
    Ok(mapped)
}
pub fn force_object(
    object: DeviceObject32,
    haptics: Option<super::haptics::HapticCapabilities32>,
) -> bool {
    haptics.is_some_and(|capabilities| match object {
        DeviceObject32::Axis { index, aspect: 0 } => index < capabilities.axes,
        DeviceObject32::Button(_) => true,
        _ => false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_keyboard_and_mouse_semantics_do_not_map_to_other_device_classes() {
        assert_eq!(
            default_object(DeviceDescription32::keyboard(), 0x8100_041e),
            Some(DeviceObject32::Key(30))
        );
        assert_eq!(
            default_object(DeviceDescription32::mouse(), 0x8200_0304),
            Some(DeviceObject32::Axis {
                index: 1,
                aspect: 0
            })
        );
        assert_eq!(
            default_object(DeviceDescription32::mouse(), 0x8100_041e),
            None
        );
    }
    #[test]
    fn force_actions_require_provisioned_actuators_and_keep_ordinary_action_mapping_separate() {
        let description = DeviceDescription32 {
            kind: DeviceKind32::Controller(0),
            axes: 2,
            povs: 1,
            buttons: 2,
        };
        let action = |semantic| Action32 {
            app_data: 0,
            semantic,
            flags: 1,
            name: 0,
            instance: [0; 16],
            object_id: 0,
            how: 0,
        };
        let original = [
            action(0x01008200),
            action(0x01010200),
            action(0x01000401),
            action(0x01000601),
        ];
        let mut actions = original;
        assert_eq!(
            build_actions(description, [1; 16], &mut actions, false).unwrap(),
            0
        );
        let caps = super::super::haptics::HapticCapabilities32 {
            axes: 1,
            max_effects: 8,
        };
        let mut actions = original;
        assert_eq!(
            build_actions_with_haptics(description, [1; 16], &mut actions, false, Some(caps))
                .unwrap(),
            2
        );
        assert_eq!(actions.map(|action| action.how), [0x20, 0, 0x20, 0]);
        assert_eq!(actions[0].object_id, 2);
        assert_eq!(actions[2].object_id, 4);
    }
}
