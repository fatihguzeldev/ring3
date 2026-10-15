mod creation;
mod locks;
mod private;

use super::super::tests::support::{DATA, read, runtime, texture};
use crate::{
    graphics::d3d8::api::InterfaceKind,
    memory::{GuestAddress, PageRange, Permissions},
};
