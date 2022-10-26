#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct HookHandle(u32);

pub struct Hook<F: ?Sized> {
    handle: HookHandle,
    //pub(super) key: String,
    pub(super) func: Box<F>,
}
impl<F: ?Sized> PartialEq for Hook<F> {
    fn eq(&self, other: &Self) -> bool {
        //self.key == other.key
        self.handle == other.handle
    }
}
impl<F: ?Sized> Eq for Hook<F> {}

pub struct HooksList<F: ?Sized> {
    handle_counter: u32,
    pub hooks: Vec<Hook<F>>,
}
// We don't want to block structures that contain a HookList from
// automatically deriving Clone, but lists themselves will be
// cleared
impl<F: ?Sized> Clone for HooksList<F> {
    fn clone(&self) -> Self {
        Default::default()
    }
}
impl<F: ?Sized> Default for HooksList<F> {
    fn default() -> Self {
        Self {
            hooks: vec![],
            handle_counter: 0,
        }
    }
}

impl<F: ?Sized> HooksList<F> {
    pub fn add_hook(&mut self, func: Box<F>) -> HookHandle {
        let handle = HookHandle(self.handle_counter);
        self.handle_counter += 1;
        self.hooks.push(Hook::<F> { handle, func });
        handle
    }

    pub fn remove_hook(&mut self, handle: HookHandle) {
        let mut index = None;
        for i in 0..self.hooks.len() {
            if self.hooks[i].handle == handle {
                index = Some(i);
                break;
            }
        }
        if let Some(index) = index {
            self.hooks.swap_remove(index);
        }
    }
}
