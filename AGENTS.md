This is a TAURI desktop app project. Because of this there are a few considerations to ensure that code quality remains high in this codebase.

- It is imperative to always ask the user whether or not a feature will live in the src-tauri (rust backend) or src (react frontend) or a combination of both.
- It is very important that when writing code to ALWAYS ground decisions in the 
[tauri documentation](https://docs.rs/tauri/2.11.5/tauri) and Rust language documentation.
When it comes to React frontend code, you can generally trust your instincts and don't need to go to documentation unless the problem is difficult or user requests you to do so.
