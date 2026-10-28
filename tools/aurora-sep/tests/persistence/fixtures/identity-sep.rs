// SPDX-License-Identifier: GPL-2.0-only OR MIT

}
struct SepData {
    capture_epoch: AtomicU64,
    bio_session: Lock<bio::Session>,
    bio_index: Lock<bio::IdentityIndex>,
    host_store: RefCell<Option<store::Store>>,
    listed: RefCell<Option<Vec<[u8; 16]>>>,
    wakes: Cell<usize>,
}
impl SepData {
    fn new() -> Rc<Self> {
        WRITES.with(|n| n.set(0));
        INDEX_LOCKS.with(|n| n.set(0));
        POLL_GAP.with(|v| v.set(false));
        GAP_RESULT.with(|v| v.set(None));
        CANCEL_AFTER_WRITE.with(|v| v.set(false));
        CLOSE_AFTER_WRITE.with(|v| v.set(false));
        FAIL_INSERT.with(|v| v.set(false));
        PUSH_BUDGET.with(|v| v.set(None));
        READ_ERROR.with(|v| v.set(false));
        let sep = Rc::new(Self {
            capture_epoch: AtomicU64::new(7),
            bio_session: Lock::new(bio::session(), 2),
            bio_index: Lock::new(bio::IdentityIndex::new(), 1),
            host_store: RefCell::new(Some(store::Store {
                raw: None,
                fail: false,
            })),
            listed: RefCell::new(None),
            wakes: Cell::new(0),
        });
        CURRENT.with(|v| *v.borrow_mut() = Some(Rc::downgrade(&sep)));
        sep
    }
    fn with_host_store<R>(&self, f: impl FnOnce(&mut store::Store) -> R) -> Option<R> {
        self.host_store.borrow_mut().as_mut().map(f)
    }
    fn bio_wake(&self) {
        self.wakes.set(self.wakes.get() + 1)
    }
    fn enclave_identities_for(&self, _: i32) -> Option<KVec<[u8; 16]>> {
        self.listed.borrow().as_ref().map(|v| KVec(v.clone()))
    }
    fn poll(&self) -> bio::EnrolPoll {
        let mut session = self.bio_session.lock();
        let mut ctx = bio::Context {
            session: &mut session,
        };
        bio::enrol_poll(&mut ctx, 0).unwrap();
        LAST_POLL.with(|v| v.borrow().clone().unwrap())
    }
    fn reboot_index(&self) -> bio::IdentityIndex {
        self.with_host_store(|s| bio::IdentityIndex::load(s).unwrap())
            .unwrap()
    }
