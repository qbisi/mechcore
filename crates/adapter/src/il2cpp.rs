use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::mem;
use std::ptr;

#[repr(C)]
pub struct Domain {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Assembly {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Image {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Class {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Object {
    _private: [u8; 0],
}
#[repr(C)]
pub struct StringObject {
    _private: [u8; 0],
}
#[repr(C)]
pub struct FieldInfo {
    _private: [u8; 0],
}
#[repr(C)]
pub struct MethodInfo {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Type {
    _private: [u8; 0],
}

type DomainGet = unsafe extern "C" fn() -> *mut Domain;
type DomainGetAssemblies =
    unsafe extern "C" fn(*const Domain, *mut usize) -> *const *const Assembly;
type AssemblyGetImage = unsafe extern "C" fn(*const Assembly) -> *const Image;
type ImageGetName = unsafe extern "C" fn(*const Image) -> *const c_char;
type ClassFromName = unsafe extern "C" fn(*const Image, *const c_char, *const c_char) -> *mut Class;
type ClassGetParent = unsafe extern "C" fn(*mut Class) -> *mut Class;
type ClassGetName = unsafe extern "C" fn(*mut Class) -> *const c_char;
type ClassGetNamespace = unsafe extern "C" fn(*mut Class) -> *const c_char;
type ClassGetType = unsafe extern "C" fn(*mut Class) -> *const Type;
type ClassGetFieldFromName = unsafe extern "C" fn(*mut Class, *const c_char) -> *mut FieldInfo;
type FieldGetValue = unsafe extern "C" fn(*mut Object, *mut FieldInfo, *mut c_void);
type FieldStaticGetValue = unsafe extern "C" fn(*mut FieldInfo, *mut c_void);
type ClassGetMethodFromName =
    unsafe extern "C" fn(*mut Class, *const c_char, c_int) -> *const MethodInfo;
type ClassGetMethods = unsafe extern "C" fn(*mut Class, *mut *mut c_void) -> *const MethodInfo;
type MethodGetName = unsafe extern "C" fn(*const MethodInfo) -> *const c_char;
type MethodGetParamCount = unsafe extern "C" fn(*const MethodInfo) -> u32;
type MethodGetParam = unsafe extern "C" fn(*const MethodInfo, u32) -> *const Type;
type TypeGetName = unsafe extern "C" fn(*const Type) -> *mut c_char;
type TypeGetObject = unsafe extern "C" fn(*const Type) -> *mut Object;
type Free = unsafe extern "C" fn(*mut c_void);
type RuntimeInvoke = unsafe extern "C" fn(
    *const MethodInfo,
    *mut c_void,
    *mut *mut c_void,
    *mut *mut Object,
) -> *mut Object;
type ObjectNew = unsafe extern "C" fn(*const Class) -> *mut Object;
type ObjectUnbox = unsafe extern "C" fn(*mut Object) -> *mut c_void;
type ObjectGetClass = unsafe extern "C" fn(*mut Object) -> *mut Class;
type ArrayLength = unsafe extern "C" fn(*mut Object) -> usize;
type ArrayObjectHeaderSize = unsafe extern "C" fn() -> usize;
type GcHandleNew = unsafe extern "C" fn(*mut Object, bool) -> u32;
type GcHandleGetTarget = unsafe extern "C" fn(u32) -> *mut Object;
type GcHandleFree = unsafe extern "C" fn(u32);
type StringNew = unsafe extern "C" fn(*const c_char) -> *mut StringObject;
type StringChars = unsafe extern "C" fn(*mut StringObject) -> *const u16;
type StringLength = unsafe extern "C" fn(*mut StringObject) -> i32;
#[cfg(not(target_os = "macos"))]
type ThreadAttach = unsafe extern "C" fn(*mut Domain) -> *mut c_void;
#[cfg(not(target_os = "macos"))]
type ThreadDetach = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
pub struct Api {
    domain_get: DomainGet,
    domain_get_assemblies: DomainGetAssemblies,
    assembly_get_image: AssemblyGetImage,
    image_get_name: ImageGetName,
    class_from_name: ClassFromName,
    class_get_parent: ClassGetParent,
    class_get_name: ClassGetName,
    class_get_namespace: ClassGetNamespace,
    class_get_type: ClassGetType,
    class_get_field_from_name: ClassGetFieldFromName,
    field_get_value: FieldGetValue,
    field_static_get_value: FieldStaticGetValue,
    class_get_method_from_name: ClassGetMethodFromName,
    class_get_methods: ClassGetMethods,
    method_get_name: MethodGetName,
    method_get_param_count: MethodGetParamCount,
    method_get_param: MethodGetParam,
    type_get_name: TypeGetName,
    type_get_object: TypeGetObject,
    free: Free,
    runtime_invoke: RuntimeInvoke,
    object_new: ObjectNew,
    object_unbox: ObjectUnbox,
    object_get_class: ObjectGetClass,
    array_length: ArrayLength,
    array_object_header_size: ArrayObjectHeaderSize,
    gchandle_new: GcHandleNew,
    gchandle_get_target: GcHandleGetTarget,
    gchandle_free: GcHandleFree,
    string_new: StringNew,
    string_chars: StringChars,
    string_length: StringLength,
    #[cfg(not(target_os = "macos"))]
    thread_attach: ThreadAttach,
    #[cfg(not(target_os = "macos"))]
    thread_detach: ThreadDetach,
}

#[derive(Debug)]
pub enum Error {
    MissingExport(&'static str),
    MissingImage(String),
    MissingClass {
        image: String,
        namespace: String,
        name: String,
    },
    MissingField(String),
    MissingMethod {
        class: String,
        method: String,
        argc: i32,
    },
    ManagedException(String),
    NullResult(String),
    InvalidCString,
    InvalidString,
    InvalidValue(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExport(name) => write!(f, "missing IL2CPP export {name}"),
            Self::MissingImage(name) => write!(f, "missing IL2CPP image {name}"),
            Self::MissingClass {
                image,
                namespace,
                name,
            } => {
                write!(f, "missing class {image}!{namespace}.{name}")
            }
            Self::MissingField(name) => write!(f, "missing field {name}"),
            Self::MissingMethod {
                class,
                method,
                argc,
            } => {
                write!(f, "missing method {class}.{method}/{argc}")
            }
            Self::ManagedException(kind) => write!(f, "managed exception {kind}"),
            Self::NullResult(context) => write!(f, "null result from {context}"),
            Self::InvalidCString => f.write_str("string contains NUL"),
            Self::InvalidString => f.write_str("invalid managed string"),
            Self::InvalidValue(detail) => f.write_str(detail),
        }
    }
}

impl std::error::Error for Error {}

impl Api {
    /// Resolve the narrow IL2CPP C API used by the adapter.
    ///
    /// # Safety
    /// The current process must contain ABI-compatible IL2CPP exports.
    pub unsafe fn load() -> Result<Self, Error> {
        unsafe fn symbol<T: Copy>(name: &'static str) -> Result<T, Error> {
            let cname = CString::new(format!("il2cpp_{name}")).expect("static export name");
            // SAFETY: dlsym is called with a valid C string. The caller validates that
            // the loaded game exposes the expected IL2CPP ABI.
            let pointer = unsafe { libc::dlsym(libc::RTLD_DEFAULT, cname.as_ptr()) };
            if pointer.is_null() {
                return Err(Error::MissingExport(name));
            }
            // SAFETY: every requested T is a pointer-sized C function pointer with
            // the signature documented by the IL2CPP C API.
            Ok(unsafe { mem::transmute_copy::<*mut c_void, T>(&pointer) })
        }

        Ok(Self {
            domain_get: unsafe { symbol("domain_get")? },
            domain_get_assemblies: unsafe { symbol("domain_get_assemblies")? },
            assembly_get_image: unsafe { symbol("assembly_get_image")? },
            image_get_name: unsafe { symbol("image_get_name")? },
            class_from_name: unsafe { symbol("class_from_name")? },
            class_get_parent: unsafe { symbol("class_get_parent")? },
            class_get_name: unsafe { symbol("class_get_name")? },
            class_get_namespace: unsafe { symbol("class_get_namespace")? },
            class_get_type: unsafe { symbol("class_get_type")? },
            class_get_field_from_name: unsafe { symbol("class_get_field_from_name")? },
            field_get_value: unsafe { symbol("field_get_value")? },
            field_static_get_value: unsafe { symbol("field_static_get_value")? },
            class_get_method_from_name: unsafe { symbol("class_get_method_from_name")? },
            class_get_methods: unsafe { symbol("class_get_methods")? },
            method_get_name: unsafe { symbol("method_get_name")? },
            method_get_param_count: unsafe { symbol("method_get_param_count")? },
            method_get_param: unsafe { symbol("method_get_param")? },
            type_get_name: unsafe { symbol("type_get_name")? },
            type_get_object: unsafe { symbol("type_get_object")? },
            free: unsafe { symbol("free")? },
            runtime_invoke: unsafe { symbol("runtime_invoke")? },
            object_new: unsafe { symbol("object_new")? },
            object_unbox: unsafe { symbol("object_unbox")? },
            object_get_class: unsafe { symbol("object_get_class")? },
            array_length: unsafe { symbol("array_length")? },
            array_object_header_size: unsafe { symbol("array_object_header_size")? },
            gchandle_new: unsafe { symbol("gchandle_new")? },
            gchandle_get_target: unsafe { symbol("gchandle_get_target")? },
            gchandle_free: unsafe { symbol("gchandle_free")? },
            string_new: unsafe { symbol("string_new")? },
            string_chars: unsafe { symbol("string_chars")? },
            string_length: unsafe { symbol("string_length")? },
            #[cfg(not(target_os = "macos"))]
            thread_attach: unsafe { symbol("thread_attach")? },
            #[cfg(not(target_os = "macos"))]
            thread_detach: unsafe { symbol("thread_detach")? },
        })
    }

    pub fn domain(self) -> Result<*mut Domain, Error> {
        // SAFETY: resolved IL2CPP API function.
        let domain = unsafe { (self.domain_get)() };
        (!domain.is_null())
            .then_some(domain)
            .ok_or_else(|| Error::NullResult("domain_get".into()))
    }

    #[cfg(not(target_os = "macos"))]
    pub fn attach(self) -> Result<ThreadGuard, Error> {
        let domain = self.domain()?;
        // SAFETY: domain belongs to the current IL2CPP runtime.
        let thread = unsafe { (self.thread_attach)(domain) };
        (!thread.is_null())
            .then_some(ThreadGuard { api: self, thread })
            .ok_or_else(|| Error::NullResult("thread_attach".into()))
    }

    pub fn image(self, wanted: &str) -> Result<*const Image, Error> {
        let domain = self.domain()?;
        let mut count = 0;
        // SAFETY: domain is live; IL2CPP owns the returned array.
        let assemblies = unsafe { (self.domain_get_assemblies)(domain, &raw mut count) };
        if assemblies.is_null() || count > 4096 {
            return Err(Error::InvalidValue("invalid assembly table".into()));
        }
        for index in 0..count {
            // SAFETY: index is within the reported table size.
            let assembly = unsafe { *assemblies.add(index) };
            if assembly.is_null() {
                continue;
            }
            // SAFETY: assembly is an IL2CPP-owned assembly pointer.
            let image = unsafe { (self.assembly_get_image)(assembly) };
            if image.is_null() {
                continue;
            }
            // SAFETY: image name is a runtime-owned NUL-terminated string.
            let raw_name = unsafe { (self.image_get_name)(image) };
            if !raw_name.is_null()
                && unsafe { CStr::from_ptr(raw_name) }.to_bytes() == wanted.as_bytes()
            {
                return Ok(image);
            }
        }
        Err(Error::MissingImage(wanted.into()))
    }

    pub fn class(self, image: &str, namespace: &str, name: &str) -> Result<*mut Class, Error> {
        let image_ptr = self.image(image)?;
        let namespace_c = CString::new(namespace).map_err(|_| Error::InvalidCString)?;
        let name_c = CString::new(name).map_err(|_| Error::InvalidCString)?;
        // SAFETY: all arguments belong to the current runtime or are valid C strings.
        let class =
            unsafe { (self.class_from_name)(image_ptr, namespace_c.as_ptr(), name_c.as_ptr()) };
        (!class.is_null())
            .then_some(class)
            .ok_or_else(|| Error::MissingClass {
                image: image.into(),
                namespace: namespace.into(),
                name: name.into(),
            })
    }

    pub fn class_name(self, class: *mut Class) -> String {
        if class.is_null() {
            return "<null>".into();
        }
        // SAFETY: class is a runtime class pointer.
        let name = unsafe { (self.class_get_name)(class) };
        if name.is_null() {
            return "<unnamed>".into();
        }
        // SAFETY: IL2CPP class names are NUL-terminated runtime strings.
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn class_namespace(self, class: *mut Class) -> String {
        if class.is_null() {
            return String::new();
        }
        // SAFETY: class is a runtime class pointer.
        let namespace = unsafe { (self.class_get_namespace)(class) };
        if namespace.is_null() {
            return String::new();
        }
        // SAFETY: IL2CPP namespaces are NUL-terminated runtime strings.
        unsafe { CStr::from_ptr(namespace) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn class_parent(self, class: *mut Class) -> Option<*mut Class> {
        if class.is_null() {
            return None;
        }
        // SAFETY: class is a runtime class pointer.
        let parent = unsafe { (self.class_get_parent)(class) };
        (!parent.is_null()).then_some(parent)
    }

    pub fn object_class(self, object: *mut Object) -> Option<*mut Class> {
        if object.is_null() {
            return None;
        }
        // SAFETY: object is checked non-null and comes from IL2CPP.
        let class = unsafe { (self.object_get_class)(object) };
        (!class.is_null()).then_some(class)
    }

    pub fn object_class_name(self, object: *mut Object) -> String {
        self.object_class(object)
            .map_or_else(|| "<null>".into(), |class| self.class_name(class))
    }

    pub fn byte_array(self, array: *mut Object) -> Result<Vec<u8>, Error> {
        if array.is_null() {
            return Err(Error::NullResult("byte array".into()));
        }
        // SAFETY: array is a managed array returned by IL2CPP.
        let length = unsafe { (self.array_length)(array) };
        if length > 64 * 1024 * 1024 {
            return Err(Error::InvalidValue(format!(
                "managed byte array exceeds 64 MiB: {length}"
            )));
        }
        // SAFETY: the runtime reports the byte offset from the object to the
        // first array element for this ABI.
        let offset = unsafe { (self.array_object_header_size)() };
        if !(std::mem::size_of::<usize>() * 3..=256).contains(&offset) {
            return Err(Error::InvalidValue(format!(
                "invalid managed array header size {offset}"
            )));
        }
        // SAFETY: byte arrays contain exactly length contiguous u8 elements
        // after the runtime-reported header.
        let bytes = unsafe { std::slice::from_raw_parts(array.cast::<u8>().add(offset), length) };
        Ok(bytes.to_vec())
    }

    pub fn value_array<T: Copy>(self, array: *mut Object, cap: usize) -> Result<Vec<T>, Error> {
        if array.is_null() {
            return Err(Error::NullResult("value array".into()));
        }
        // SAFETY: array is a managed one-dimensional value-type array returned by IL2CPP.
        let length = unsafe { (self.array_length)(array) };
        if length > cap {
            return Err(Error::InvalidValue(format!(
                "managed value array length {length} exceeds {cap}"
            )));
        }
        // SAFETY: the runtime reports the byte offset from the object to the first element.
        let offset = unsafe { (self.array_object_header_size)() };
        if !(std::mem::size_of::<usize>() * 3..=256).contains(&offset) {
            return Err(Error::InvalidValue(format!(
                "invalid managed array header size {offset}"
            )));
        }
        // SAFETY: the caller binds T to the managed array element type; elements are contiguous.
        let values = unsafe {
            std::slice::from_raw_parts(array.cast::<u8>().add(offset).cast::<T>(), length)
        };
        Ok(values.to_vec())
    }

    pub fn gc_handle(self, object: *mut Object) -> Result<u32, Error> {
        if object.is_null() {
            return Err(Error::NullResult("GC handle target".into()));
        }
        // SAFETY: object belongs to the current runtime; a non-pinned strong
        // handle keeps it alive without exposing its storage address contract.
        let handle = unsafe { (self.gchandle_new)(object, false) };
        (handle != 0)
            .then_some(handle)
            .ok_or_else(|| Error::NullResult("gchandle_new".into()))
    }

    pub fn gc_handle_target(self, handle: u32) -> Result<*mut Object, Error> {
        // SAFETY: callers pass a live handle created by gc_handle.
        let object = unsafe { (self.gchandle_get_target)(handle) };
        (!object.is_null())
            .then_some(object)
            .ok_or_else(|| Error::NullResult("gchandle_get_target".into()))
    }

    pub fn free_gc_handle(self, handle: u32) {
        if handle != 0 {
            // SAFETY: callers free each owned handle at most once.
            unsafe { (self.gchandle_free)(handle) };
        }
    }

    pub fn field(self, class: *mut Class, name: &str) -> Result<*mut FieldInfo, Error> {
        let name_c = CString::new(name).map_err(|_| Error::InvalidCString)?;
        let mut current = class;
        while !current.is_null() {
            // SAFETY: current is a runtime class and name_c is a valid C string.
            let field = unsafe { (self.class_get_field_from_name)(current, name_c.as_ptr()) };
            if !field.is_null() {
                return Ok(field);
            }
            // SAFETY: current is a runtime class.
            current = unsafe { (self.class_get_parent)(current) };
        }
        Err(Error::MissingField(name.into()))
    }

    pub fn class_is_or_inherits(self, mut class: *mut Class, expected: *mut Class) -> bool {
        if expected.is_null() {
            return false;
        }
        while !class.is_null() {
            if class == expected {
                return true;
            }
            // SAFETY: class is a runtime class from the current IL2CPP domain.
            class = unsafe { (self.class_get_parent)(class) };
        }
        false
    }

    pub fn static_object(self, field: *mut FieldInfo) -> *mut Object {
        let mut value: *mut Object = ptr::null_mut();
        // SAFETY: field is a static object field and output points to pointer storage.
        unsafe { (self.field_static_get_value)(field, (&raw mut value).cast()) };
        value
    }

    pub fn field_value<T: Copy>(
        self,
        object: *mut Object,
        field: *mut FieldInfo,
    ) -> Result<T, Error> {
        if object.is_null() || field.is_null() {
            return Err(Error::NullResult("field value".into()));
        }
        let mut value = mem::MaybeUninit::<T>::uninit();
        // SAFETY: object and field belong to the current runtime; the caller
        // supplies the field's ABI-compatible value type.
        unsafe { (self.field_get_value)(object, field, value.as_mut_ptr().cast()) };
        // SAFETY: IL2CPP initialized the complete field value above.
        Ok(unsafe { value.assume_init() })
    }

    pub fn method(
        self,
        mut class: *mut Class,
        name: &str,
        argc: i32,
    ) -> Result<*const MethodInfo, Error> {
        let original = self.class_name(class);
        let name_c = CString::new(name).map_err(|_| Error::InvalidCString)?;
        while !class.is_null() {
            // SAFETY: class is a runtime class and name_c is a valid C string.
            let method = unsafe { (self.class_get_method_from_name)(class, name_c.as_ptr(), argc) };
            if !method.is_null() {
                return Ok(method);
            }
            // SAFETY: class is a runtime class.
            class = unsafe { (self.class_get_parent)(class) };
        }
        Err(Error::MissingMethod {
            class: original,
            method: name.into(),
            argc,
        })
    }

    #[allow(clippy::unused_self)] // Kept on Api because it exposes IL2CPP MethodInfo layout.
    pub fn method_pointer(self, method: *const MethodInfo) -> Result<*mut c_void, Error> {
        if method.is_null() {
            return Err(Error::NullResult("method pointer".into()));
        }
        // SAFETY: IL2CPP MethodInfo begins with the generated native method pointer.
        let target = unsafe { method.cast::<*mut c_void>().read() };
        (!target.is_null())
            .then_some(target)
            .ok_or_else(|| Error::NullResult("method pointer".into()))
    }

    pub fn find_object_of_class(self, class: *mut Class) -> Result<*mut Object, Error> {
        // SAFETY: class belongs to the current runtime.
        let runtime_type = unsafe { (self.class_get_type)(class) };
        if runtime_type.is_null() {
            return Err(Error::NullResult("class_get_type".into()));
        }
        // SAFETY: runtime_type is owned by IL2CPP.
        let type_object = unsafe { (self.type_get_object)(runtime_type) };
        if type_object.is_null() {
            return Err(Error::NullResult("type_get_object".into()));
        }
        let object_class = self.class("UnityEngine.CoreModule.dll", "UnityEngine", "Object")?;
        let finder = self.class_method_with_parameter_types(
            object_class,
            "FindObjectOfType",
            &["System.Type"],
        )?;
        let found =
            self.invoke_raw(finder, ptr::null_mut(), &mut [object_argument(type_object)])?;
        (!found.is_null()).then_some(found).ok_or_else(|| {
            Error::NullResult(format!("FindObjectOfType({})", self.class_name(class)))
        })
    }

    pub fn method_with_parameter_types(
        self,
        object: *mut Object,
        name: &str,
        parameter_types: &[&str],
    ) -> Result<*const MethodInfo, Error> {
        let class = self
            .object_class(object)
            .ok_or_else(|| Error::NullResult(name.into()))?;
        self.class_method_with_parameter_types(class, name, parameter_types)
    }

    pub fn class_method_with_parameter_types(
        self,
        mut class: *mut Class,
        name: &str,
        parameter_types: &[&str],
    ) -> Result<*const MethodInfo, Error> {
        let original = self.class_name(class);
        while !class.is_null() {
            let mut iterator = ptr::null_mut();
            loop {
                // SAFETY: class is a runtime class and iterator follows the IL2CPP
                // class_get_methods enumeration contract.
                let method = unsafe { (self.class_get_methods)(class, &raw mut iterator) };
                if method.is_null() {
                    break;
                }
                // SAFETY: method belongs to the enumerated runtime class.
                let raw_name = unsafe { (self.method_get_name)(method) };
                if raw_name.is_null()
                    || unsafe { CStr::from_ptr(raw_name) }.to_bytes() != name.as_bytes()
                    || unsafe { (self.method_get_param_count)(method) }
                        != u32::try_from(parameter_types.len()).unwrap_or(u32::MAX)
                {
                    continue;
                }
                let mut matches = true;
                for (index, expected) in parameter_types.iter().enumerate() {
                    // SAFETY: index is below the parameter count checked above.
                    let parameter = unsafe {
                        (self.method_get_param)(method, u32::try_from(index).unwrap_or(u32::MAX))
                    };
                    if parameter.is_null() {
                        matches = false;
                        break;
                    }
                    // SAFETY: parameter belongs to method; IL2CPP allocates the
                    // returned name and requires il2cpp_free.
                    let raw_type = unsafe { (self.type_get_name)(parameter) };
                    if raw_type.is_null() {
                        matches = false;
                        break;
                    }
                    // SAFETY: raw_type is a NUL-terminated IL2CPP-owned allocation.
                    let actual = unsafe { CStr::from_ptr(raw_type) };
                    matches = actual.to_bytes() == expected.as_bytes();
                    // SAFETY: raw_type was allocated by il2cpp_type_get_name.
                    unsafe { (self.free)(raw_type.cast()) };
                    if !matches {
                        break;
                    }
                }
                if matches {
                    return Ok(method);
                }
            }
            // SAFETY: class is a runtime class.
            class = unsafe { (self.class_get_parent)(class) };
        }
        Err(Error::MissingMethod {
            class: original,
            method: format!("{name}({})", parameter_types.join(", ")),
            argc: i32::try_from(parameter_types.len()).unwrap_or(i32::MAX),
        })
    }

    pub fn invoke_raw(
        self,
        method: *const MethodInfo,
        this: *mut c_void,
        arguments: &mut [*mut c_void],
    ) -> Result<*mut Object, Error> {
        let mut exception = ptr::null_mut();
        let args = if arguments.is_empty() {
            ptr::null_mut()
        } else {
            arguments.as_mut_ptr()
        };
        // SAFETY: method and this are runtime pointers; arguments contains pointers
        // to ABI-compatible value slots for the selected method.
        let result = unsafe { (self.runtime_invoke)(method, this, args, &raw mut exception) };
        if !exception.is_null() {
            return Err(Error::ManagedException(self.object_class_name(exception)));
        }
        Ok(result)
    }

    pub fn invoke(
        self,
        object: *mut Object,
        name: &str,
        arguments: &mut [*mut c_void],
    ) -> Result<*mut Object, Error> {
        let class = self
            .object_class(object)
            .ok_or_else(|| Error::NullResult(name.into()))?;
        let method = self.method(
            class,
            name,
            i32::try_from(arguments.len()).unwrap_or(i32::MAX),
        )?;
        self.invoke_raw(method, object.cast(), arguments)
    }

    pub fn invoke_static(
        self,
        class: *mut Class,
        name: &str,
        arguments: &mut [*mut c_void],
    ) -> Result<*mut Object, Error> {
        let method = self.method(
            class,
            name,
            i32::try_from(arguments.len()).unwrap_or(i32::MAX),
        )?;
        self.invoke_raw(method, ptr::null_mut(), arguments)
    }

    pub fn invoke_value<T: Copy>(
        self,
        object: *mut Object,
        name: &str,
        arguments: &mut [*mut c_void],
    ) -> Result<T, Error> {
        let boxed = self.invoke(object, name, arguments)?;
        self.unbox(boxed, name)
    }

    pub fn invoke_void(
        self,
        object: *mut Object,
        name: &str,
        arguments: &mut [*mut c_void],
    ) -> Result<(), Error> {
        self.invoke(object, name, arguments).map(|_| ())
    }

    pub fn unbox<T: Copy>(self, boxed: *mut Object, context: &str) -> Result<T, Error> {
        if boxed.is_null() {
            return Err(Error::NullResult(context.into()));
        }
        // SAFETY: boxed is a non-null boxed value returned by IL2CPP.
        let value = unsafe { (self.object_unbox)(boxed) }.cast::<T>();
        if value.is_null() {
            return Err(Error::NullResult(format!("unbox {context}")));
        }
        // SAFETY: T is the caller-declared ABI return type of the invoked method.
        Ok(unsafe { value.read_unaligned() })
    }

    pub fn new_object(self, class: *mut Class) -> Result<*mut Object, Error> {
        let object = self.allocate_object(class)?;
        self.invoke_void(object, ".ctor", &mut [])?;
        Ok(object)
    }

    pub fn allocate_object(self, class: *mut Class) -> Result<*mut Object, Error> {
        // SAFETY: class is a runtime class from the current domain.
        let object = unsafe { (self.object_new)(class) };
        (!object.is_null())
            .then_some(object)
            .ok_or_else(|| Error::NullResult("object_new".into()))
    }

    pub fn string(self, value: &str) -> Result<*mut StringObject, Error> {
        let value = CString::new(value).map_err(|_| Error::InvalidCString)?;
        // SAFETY: value is a valid NUL-terminated UTF-8 string.
        let string = unsafe { (self.string_new)(value.as_ptr()) };
        (!string.is_null())
            .then_some(string)
            .ok_or_else(|| Error::NullResult("string_new".into()))
    }

    pub fn string_to_rust(self, value: *mut StringObject) -> Result<String, Error> {
        if value.is_null() {
            return Err(Error::InvalidString);
        }
        // SAFETY: value is a managed string.
        let length = unsafe { (self.string_length)(value) };
        if !(0..=1_048_576).contains(&length) {
            return Err(Error::InvalidString);
        }
        // SAFETY: IL2CPP owns at least length UTF-16 code units.
        let chars = unsafe { (self.string_chars)(value) };
        if chars.is_null() {
            return Err(Error::InvalidString);
        }
        // SAFETY: length was checked and chars is non-null.
        let length = usize::try_from(length).map_err(|_| Error::InvalidString)?;
        let slice = unsafe { std::slice::from_raw_parts(chars, length) };
        String::from_utf16(slice).map_err(|_| Error::InvalidString)
    }

    pub fn unboxed_this(self, boxed: *mut Object) -> Result<*mut c_void, Error> {
        if boxed.is_null() {
            return Err(Error::NullResult("boxed value".into()));
        }
        // SAFETY: boxed is a managed boxed value.
        let value = unsafe { (self.object_unbox)(boxed) };
        (!value.is_null())
            .then_some(value)
            .ok_or_else(|| Error::NullResult("object_unbox".into()))
    }
}

#[cfg(not(target_os = "macos"))]
pub struct ThreadGuard {
    api: Api,
    thread: *mut c_void,
}

#[cfg(not(target_os = "macos"))]
impl Drop for ThreadGuard {
    fn drop(&mut self) {
        // SAFETY: thread was returned by thread_attach on this API.
        unsafe { (self.api.thread_detach)(self.thread) };
    }
}

pub fn argument<T>(value: &mut T) -> *mut c_void {
    ptr::from_mut(value).cast()
}

pub fn object_argument<T>(value: *mut T) -> *mut c_void {
    value.cast()
}
