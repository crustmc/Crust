use wasmer::MemoryView;

pub type WPtr<T> = wasmer::WasmPtr<T>;

pub fn cast_value<'a, T>(view: &MemoryView<'a>, ptr: &WPtr<T>) -> &'a T {
    unsafe { &*(view.data_unchecked().as_ptr().add(ptr.offset() as usize) as *const T) }
}

pub fn cast_value_mut<'a, T>(view: &MemoryView<'a>, ptr: &WPtr<T>) -> &'a mut T {
    unsafe { &mut *(view.data_unchecked_mut().as_ptr().add(ptr.offset() as usize) as *mut T) }
}

pub fn cast_value_array<'a, T>(view: &MemoryView<'a>, ptr: &WPtr<T>, len: usize) -> &'a [T] {
    unsafe { std::slice::from_raw_parts(view.data_unchecked().as_ptr().add(ptr.offset() as usize) as *const T, len) }
}

pub fn cast_value_array_mut<'a, T>(view: &MemoryView<'a>, ptr: &WPtr<T>, len: usize) -> &'a mut [T] {
    unsafe { std::slice::from_raw_parts_mut(view.data_unchecked_mut().as_ptr().add(ptr.offset() as usize) as *mut T, len) }
}

pub fn cast_str<'a>(view: &MemoryView<'a>, ptr: &WPtr<u8>, len: &WPtr<()>) -> &'a str {
    unsafe {
        let slice = &view.data_unchecked()[ptr.offset() as usize..];
        let len = (len.offset() as usize).min(slice.len());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(slice.as_ptr(), len))
    }
}

#[repr(C)]
pub struct PluginMetadata {
    pub sdk_version: u32,
    pub manifest: WPtr<u8>,
    pub manifest_len: WPtr<()>,
}

impl PluginMetadata {
    
    pub fn get_manifest<'a>(&self, view: &MemoryView<'a>) -> &'a str {
        cast_str(view, &self.manifest, &self.manifest_len)
    }
}
