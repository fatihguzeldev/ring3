use crate::Observer;
use std::{cell::RefCell, marker::PhantomData, rc::Rc};

thread_local! {
    static CURRENT: RefCell<(u64, Vec<(u64, Observer)>)> = const { RefCell::new((0, Vec::new())) };
}

pub struct Context {
    token: Option<u64>,
    thread_bound: PhantomData<Rc<()>>,
}

impl Observer {
    pub fn enter(&self) -> Context {
        let token = CURRENT.with(|current| {
            let mut current = current.borrow_mut();
            if !self.is_recording() && current.1.is_empty() {
                return None;
            }
            current.0 = current
                .0
                .checked_add(1)
                .expect("trace context identity exhausted");
            let token = current.0;
            current.1.push((token, self.clone()));
            Some(token)
        });
        Context {
            token,
            thread_bound: PhantomData,
        }
    }
}

pub fn current() -> Observer {
    CURRENT.with(|current| {
        current
            .borrow()
            .1
            .last()
            .map_or_else(Observer::disabled, |(_, observer)| observer.clone())
    })
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            CURRENT.with(|current| {
                current.borrow_mut().1.retain(|(id, _)| *id != token);
            });
        }
    }
}
