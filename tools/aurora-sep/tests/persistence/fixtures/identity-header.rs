// SPDX-License-Identifier: GPL-2.0-only OR MIT
#![allow(dead_code, unused_variables, unused_imports)]
use std::cell::{Cell, RefCell, RefMut};
use std::ffi::CStr;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
type Result<T> = std::result::Result<T, i32>;
const EINVAL: i32 = 22;
const ENOSPC: i32 = 28;
const EBUSY: i32 = 16;
const ENOENT: i32 = 2;
const GFP_KERNEL: u32 = 0;
const ENROL_STATUS_UNFILED: u32 = 6;
const SBIO_PROBE_USER_ID: i32 = 1000;
macro_rules! dev_err {
    ($($a:tt)*) => {{
        ()
    }};
}
macro_rules! dev_warn {
    ($($a:tt)*) => {{
        ()
    }};
}
#[derive(Clone, Debug)]
struct KVec<T>(Vec<T>);
impl<T> KVec<T> {
    fn new() -> Self {
        Self(vec![])
    }
    fn push(&mut self, v: T, _: u32) -> Result<()> {
        if PUSH_BUDGET.with(|b| match b.get() {
            None => false,
            Some(0) => true,
            Some(n) => {
                b.set(Some(n - 1));
                false
            }
        }) {
            return Err(12);
        }
        self.0.push(v);
        Ok(())
    }
    fn extend_from_slice(&mut self, v: &[T], _: u32) -> Result<()>
    where
        T: Clone,
    {
        self.0.extend_from_slice(v);
        Ok(())
    }
}
impl<T> Deref for KVec<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}
impl<T> DerefMut for KVec<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}
#[derive(Clone, Copy)]
struct Key;
impl Key {
    fn root(_: u8) -> Self {
        Self
    }
}
thread_local! {
 static CURRENT:RefCell<Option<Weak<SepData>>>=const{RefCell::new(None)};
 static INDEX_LOCKS:Cell<usize>=const{Cell::new(0)};
 static POLL_GAP:Cell<bool>=const{Cell::new(false)};
 static GAP_RESULT:Cell<Option<(u32,usize)>>=const{Cell::new(None)};
 static CANCEL_AFTER_WRITE:Cell<bool>=const{Cell::new(false)};
 static CLOSE_AFTER_WRITE:Cell<bool>=const{Cell::new(false)};
 static WRITES:Cell<usize>=const{Cell::new(0)};
 static LAST_POLL:RefCell<Option<bio::EnrolPoll>>=const{RefCell::new(None)};
 static FAIL_INSERT:Cell<bool>=const{Cell::new(false)};
 static PUSH_BUDGET:Cell<Option<usize>>=const{Cell::new(None)};
 static READ_ERROR:Cell<bool>=const{Cell::new(false)};
}
struct Lock<T> {
    inner: RefCell<T>,
    kind: u8,
}
impl<T> Lock<T> {
    fn new(v: T, kind: u8) -> Self {
        Self {
            inner: RefCell::new(v),
            kind,
        }
    }
    fn lock(&self) -> RefMut<'_, T> {
        if self.kind == 1 {
            let count = INDEX_LOCKS.with(|n| {
                n.set(n.get() + 1);
                n.get()
            });
            if count == 2 && POLL_GAP.with(|v| v.replace(false)) {
                let sep = CURRENT.with(|v| v.borrow().as_ref().unwrap().upgrade().unwrap());
                if let Ok(mut session) = sep.bio_session.inner.try_borrow_mut() {
                    let mut ctx = bio::Context {
                        session: &mut session,
                    };
                    bio::enrol_poll(&mut ctx, 0).unwrap();
                    let state = LAST_POLL.with(|v| v.borrow().as_ref().unwrap().state as u32);
                    GAP_RESULT.with(|v| v.set(Some((state, WRITES.with(|w| w.get())))));
                };
            }
        }
        if self.kind == 2 && WRITES.with(|n| n.get() > 0) {
            if CANCEL_AFTER_WRITE.with(|v| v.replace(false)) {
                let sep = CURRENT.with(|v| v.borrow().as_ref().unwrap().upgrade().unwrap());
                let mut session = sep.bio_session.inner.borrow_mut();
                let mut ctx = bio::Context {
                    session: &mut session,
                };
                let _ = bio::cancel(&mut ctx);
            }
            if CLOSE_AFTER_WRITE.with(|v| v.replace(false)) {
                let sep = CURRENT.with(|v| v.borrow().as_ref().unwrap().upgrade().unwrap());
                let mut session = sep.bio_session.inner.borrow_mut();
                session.open = false;
                session.generation += 1;
                session.op = bio::Op::Idle;
            }
        }
        self.inner.borrow_mut()
    }
}
mod store {
    use super::*;
    pub struct Store {
        pub raw: Option<KVec<u8>>,
        pub fail: bool,
    }
    impl Store {
        pub fn read(&mut self, _: &Key) -> Result<Option<KVec<u8>>> {
            if READ_ERROR.with(|v| v.get()) {
                Err(5)
            } else {
                Ok(self.raw.clone())
            }
        }
        pub fn write(&mut self, _: &Key, bytes: &[u8]) -> Result<()> {
            WRITES.with(|n| n.set(n.get() + 1));
            if self.fail {
                return Err(5);
            }
            self.raw = Some(KVec(bytes.to_vec()));
            Ok(())
        }
    }
}
mod sbio {
    pub struct IdentityV1;
    impl IdentityV1 {
        pub fn from_index(_: i32, _: [u8; 16]) -> Self {
            Self
        }
    }
}
mod bio {
    use super::*;
    use store::Store;
    pub const UUID_LEN: usize = 16;
    pub const LABEL_LEN: usize = 128;
    const MAX_IDENTITIES: usize = 32;
    const INDEX_FORMAT: u32 = 2;
    const PRIVATE_TYPE_IDENTITIES: u8 = 0xf1;
    const STATUS_LOCAL: u32 = 0;
    const ENROL_STAGES: u32 = 32;
    fn identity_key() -> Key {
        Key::root(PRIVATE_TYPE_IDENTITIES)
    }
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Identity {
        pub uuid: [u8; UUID_LEN],
        pub label: [u8; LABEL_LEN],
    }
    impl Identity {
        const ZERO: Identity = Identity {
            uuid: [0; UUID_LEN],
            label: [0; LABEL_LEN],
        };
    }
    pub struct IdentityIndex {
        entries: KVec<Identity>,
    }
