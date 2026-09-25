//! HTTP 面测试门面（R1 2026-09-25 拆分）
//!
//! - `req_tests`：请求体跨包读取 / Content-Length / chunked
//! - `auth_tests`：IP 允许名单 / 退避 / LoginGuard / 门票 / bearer
//! - `fuzz_tests`：固定种子确定性模糊测试

#[path = "http_tests/req_tests.rs"]
mod req_tests;

#[path = "http_tests/auth_tests.rs"]
mod auth_tests;

#[path = "http_tests/fuzz_tests.rs"]
mod fuzz_tests;
