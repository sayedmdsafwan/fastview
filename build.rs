fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("ProductName", "FastView");
        res.set("FileDescription", "FastView - instant photo viewer");
        res.set("LegalCopyright", "MIT License");
        res.set("OriginalFilename", "fastview.exe");
        res.compile().unwrap();
    }
}
