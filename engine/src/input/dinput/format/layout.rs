use super::super::DirectInputResult32;
use super::{
    DeviceDescription32, DeviceObject32, ObjectSelector32, ObjectType32, objects::matches,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatObject32 {
    pub offset: u32,
    pub selector: ObjectSelector32,
    pub kind: ObjectType32,
    pub instance: Option<u16>,
    pub aspect: u8,
    pub optional: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatBinding32 {
    pub offset: u32,
    pub object: Option<DeviceObject32>,
    pub width: u32,
    pub app_data: u32,
    pub default_value: u32,
}
#[derive(Debug)]
pub struct DataFormat32 {
    pub size: u32,
    pub relative: bool,
    bindings: Vec<FormatBinding32>,
}
impl DataFormat32 {
    pub fn new(
        description: DeviceDescription32,
        size: u32,
        relative: bool,
        objects: &[FormatObject32],
    ) -> Result<Self, DirectInputResult32> {
        description.validate()?;
        if size > 65536
            || !size.is_multiple_of(4)
            || size == 0 && !objects.is_empty()
            || objects.len() > 1024
        {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        let mut bindings: Vec<FormatBinding32> = Vec::new();
        bindings
            .try_reserve_exact(objects.len())
            .map_err(|_| DirectInputResult32::OUT_OF_MEMORY)?;
        for requested in objects {
            if requested.aspect > 3 {
                return Err(DirectInputResult32::INVALID_PARAM);
            }
            let object = description
                .objects()
                .map(|object| match object {
                    DeviceObject32::Axis { index, .. }
                        if matches!(
                            description.kind,
                            super::super::DeviceKind32::Controller(_)
                        ) =>
                    {
                        DeviceObject32::Axis {
                            index,
                            aspect: requested.aspect,
                        }
                    }
                    _ => object,
                })
                .find(|object| {
                    matches(
                        *object,
                        requested.selector,
                        requested.kind,
                        requested.instance,
                        requested.aspect,
                    ) && !bindings.iter().any(|bound| bound.object == Some(*object))
                });
            if object.is_none() && !requested.optional {
                return Err(DirectInputResult32::INVALID_PARAM);
            }
            let width = object.map_or(
                if requested.kind == ObjectType32::Button {
                    1
                } else {
                    4
                },
                DeviceObject32::width,
            );
            let end = requested
                .offset
                .checked_add(width)
                .filter(|end| *end <= size)
                .ok_or(DirectInputResult32::INVALID_PARAM)?;
            if bindings
                .iter()
                .any(|bound| requested.offset < bound.offset + bound.width && bound.offset < end)
            {
                return Err(DirectInputResult32::INVALID_PARAM);
            }
            bindings.push(FormatBinding32 {
                offset: requested.offset,
                object,
                width,
                app_data: 0,
                default_value: if requested.kind == ObjectType32::Pov {
                    u32::MAX
                } else {
                    0
                },
            });
        }
        Ok(Self {
            size,
            relative,
            bindings,
        })
    }
    pub fn from_actions(
        description: DeviceDescription32,
        size: u32,
        relative: bool,
        actions: &[(u32, DeviceObject32, u32)],
    ) -> Result<Self, DirectInputResult32> {
        if size > 65536 || !size.is_multiple_of(4) {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(actions.len())
            .map_err(|_| DirectInputResult32::OUT_OF_MEMORY)?;
        for &(offset, object, app_data) in actions {
            if offset.checked_add(4).is_none_or(|end| end > size)
                || !description.objects().any(|candidate| candidate == object)
            {
                return Err(DirectInputResult32::INVALID_PARAM);
            }
            bindings.push(FormatBinding32 {
                offset,
                object: Some(object),
                width: 4,
                app_data,
                default_value: 0,
            });
        }
        Ok(Self {
            size,
            relative,
            bindings,
        })
    }
    pub fn bindings(&self) -> &[FormatBinding32] {
        &self.bindings
    }
    pub fn bindings_mut(&mut self) -> &mut [FormatBinding32] {
        &mut self.bindings
    }
    pub fn object_by_offset(&self, offset: u32) -> Option<DeviceObject32> {
        self.bindings
            .iter()
            .find(|binding| binding.offset == offset)
            .and_then(|binding| binding.object)
    }
    pub fn offset_for(&self, object: DeviceObject32) -> Option<u32> {
        self.bindings
            .iter()
            .find(|binding| binding.object == Some(object))
            .map(|binding| binding.offset)
    }
}
