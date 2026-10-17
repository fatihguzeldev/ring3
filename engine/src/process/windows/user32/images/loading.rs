use super::{ImageReply32, context::ImageOrigin32, input::ImageInput32};
use crate::{
    files::FileIo32,
    loader::ResourceIdentifier32 as Id,
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            gdi::GdiDibSource32,
            strings::{Input32, WindowsStringError32},
        },
    },
    windows::{
        CallFrame32,
        user32::{
            User32Api32 as Api, User32Error32 as E,
            images::{FileImage32, ImageExtent32, ImageKind32, LoadImage32, StockImage32},
        },
    },
};

struct ImageCall32 {
    owner: PendingOwner,
    token: u32,
    api: Api,
    frame: CallFrame32,
}

pub(super) fn load_request(api: Api, args: &[u32]) -> Result<(u32, u32, LoadImage32), HostError> {
    let request = match api {
        Api::LoadImageA | Api::LoadImageW => LoadImage32::new(
            ImageKind32::try_from(args[2]).map_err(HostError::User32)?,
            args[3] as i32,
            args[4] as i32,
            args[5],
        ),
        Api::LoadBitmapA | Api::LoadBitmapW => LoadImage32::new(ImageKind32::Bitmap, 0, 0, 0),
        Api::LoadIconA | Api::LoadIconW => LoadImage32::new(ImageKind32::Icon, 0, 0, 0x8040),
        Api::LoadCursorA | Api::LoadCursorW => LoadImage32::new(ImageKind32::Cursor, 0, 0, 0x8040),
        _ => return Err(HostError::InvalidRequest),
    }
    .map_err(HostError::User32)?;
    Ok((args[0], args[1], request))
}

impl EngineInstance {
    pub(super) fn load_user32_image(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: Api,
        frame: CallFrame32,
    ) -> Result<Option<u32>, HostError> {
        let (instance, name, request) = load_request(api, frame.arguments())?;
        if request.from_file() {
            return self.load_user32_image_file(
                ImageCall32 {
                    owner,
                    token,
                    api,
                    frame,
                },
                instance,
                name,
                request,
            );
        }
        let name = self.user32_resource_identifier(name, api.wide())?;
        let budget = self.image_byte_limit()?;
        let defaults = Self::image_default_extent();
        if instance == 0 {
            let Id::Id(id) = name else {
                return Err(HostError::User32(E::Win32(1814)));
            };
            let id = u16::try_from(id).map_err(|_| HostError::User32(E::Win32(1814)))?;
            if !request.shared() && !matches!(api, Api::LoadBitmapA | Api::LoadBitmapW) {
                return Err(HostError::User32(E::Win32(1814)));
            }
            let origin = ImageOrigin32::System {
                kind: request.kind,
                id,
            };
            if request.shared()
                && let Some(handle) = self.user32.as_ref().unwrap().images.shared(&origin)
            {
                self.check_user32_image(handle, request.kind)?;
                return Ok(Some(handle));
            }
            let stock = StockImage32::load(
                request.kind,
                id,
                &self.gdi.as_ref().unwrap().system_colors(),
            )
            .map_err(HostError::User32)?;
            let extent = request.extent(stock.extent, defaults);
            return self
                .publish_user32_image(
                    GdiDibSource32 {
                        info: &stock.info,
                        pixels: None,
                    },
                    request,
                    extent,
                    stock.hotspot,
                    Some(origin),
                    None,
                )
                .map(Some);
        }
        if request.shared() {
            let module = self.resource_module_identity(instance)?;
            if let Some(handle) =
                self.user32
                    .as_ref()
                    .unwrap()
                    .images
                    .shared_resource(module, request.kind, &name)
            {
                self.check_user32_image(handle, request.kind)?;
                return Ok(Some(handle));
            }
        }
        let pixels = self.read_user32_image_resource(instance, name, request, defaults, budget)?;
        let origin = ImageOrigin32::Resource(pixels.source);
        self.publish_user32_image(
            GdiDibSource32 {
                info: &pixels.bytes[pixels.offset..],
                pixels: None,
            },
            request,
            pixels.extent,
            pixels.hotspot,
            Some(origin),
            None,
        )
        .map(Some)
    }
    fn load_user32_image_file(
        &mut self,
        call: ImageCall32,
        instance: u32,
        name: u32,
        request: LoadImage32,
    ) -> Result<Option<u32>, HostError> {
        if instance != 0 {
            return Err(HostError::User32(E::Win32(87)));
        }
        let ImageCall32 {
            owner,
            token,
            api,
            frame,
        } = call;
        let thread = self.current_windows_thread_id()?;
        let mut reply = if let Some(reply) = self.user32.as_mut().unwrap().images.reply.take() {
            if reply.owner != owner
                || reply.token != token
                || reply.frame != frame
                || reply.api != api
                || reply.thread != thread
            {
                self.user32.as_mut().unwrap().images.reply = Some(reply);
                return Err(HostError::Call(CallError::Busy));
            }
            reply
        } else {
            let path = Input32::read(
                self.memory()?,
                name,
                api.wide(),
                self.windows_ansi_code_page(),
                32768,
            )
            .map_err(|error| match error {
                WindowsStringError32::Memory(error) => HostError::Memory(error),
                WindowsStringError32::Allocation => HostError::User32(E::Allocation),
                WindowsStringError32::FilenameTooLong => HostError::User32(E::Win32(206)),
                WindowsStringError32::InvalidParameter => HostError::User32(E::Win32(87)),
            })?
            .units;
            let lease = self.open_provider_file(&path, false)?;
            let length = self
                .provider_file_size(lease)
                .expect("new image source lease is current under the same coordinator");
            ImageReply32 {
                owner,
                token,
                frame,
                api,
                thread,
                request,
                input: ImageInput32::new(lease, length),
                pending: None,
                error: None,
            }
        };
        reply.pending = None;
        let result = self.poll_user32_image_file(&mut reply);
        match result {
            Ok(None) => {
                self.user32.as_mut().unwrap().images.reply = Some(reply);
                Ok(None)
            }
            Ok(Some(Ok(value))) => Ok(Some(value)),
            Ok(Some(Err(error))) => Err(HostError::User32(E::Win32(error))),
            Err(error) => {
                self.user32.as_mut().unwrap().images.reply = Some(reply);
                Err(error)
            }
        }
    }
    fn poll_user32_image_file(
        &mut self,
        reply: &mut ImageReply32,
    ) -> Result<Option<Result<u32, u32>>, HostError> {
        if reply.error.is_none() {
            let limit = self.image_byte_limit()?;
            match reply.input.poll(self, limit) {
                Ok(FileIo32::NeedBytes(request)) => {
                    reply.pending = Some(request);
                    return Ok(None);
                }
                Ok(FileIo32::Ready(())) => {}
                Err(HostError::File(e)) => reply.error = Some(e.win32()),
                Err(HostError::User32(E::Capacity | E::Allocation)) => reply.error = Some(8),
                Err(HostError::User32(E::Win32(error))) => reply.error = Some(error),
                Err(error) => return Err(error),
            }
        }
        self.check_close_provider_files(&[reply.input.lease])?;
        let result = if let Some(error) = reply.error {
            Err(HostError::User32(E::Win32(error)))
        } else {
            (|| {
                let image = FileImage32::select(
                    reply.input.bytes(),
                    reply.request,
                    Self::image_default_extent(),
                    32,
                )
                .map_err(HostError::User32)?;
                self.publish_user32_image(
                    GdiDibSource32 {
                        info: image.info,
                        pixels: image.pixels,
                    },
                    reply.request,
                    image.extent,
                    image.hotspot,
                    None,
                    None,
                )
            })()
        };
        match result {
            Ok(handle) => {
                self.commit_close_provider_files(&[reply.input.lease]);
                Ok(Some(Ok(handle)))
            }
            Err(error) => {
                let error = super::calls::failure_code(error)?;
                self.commit_close_provider_files(&[reply.input.lease]);
                Ok(Some(Err(error)))
            }
        }
    }
    pub(super) fn image_byte_limit(&self) -> Result<usize, HostError> {
        Ok(self
            .gdi
            .as_ref()
            .ok_or(HostError::User32(E::Win32(50)))?
            .config()
            .bitmap_bytes)
    }
    pub(super) fn image_default_extent() -> ImageExtent32 {
        ImageExtent32 {
            width: 32,
            height: 32,
        }
    }
}
