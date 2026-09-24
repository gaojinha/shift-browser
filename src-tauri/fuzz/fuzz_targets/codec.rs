//! 小型字符串解析面模糊目标：
//! percent-decode、form-urlencoded 取值、Content-Length、chunked 识别。
//! 任意输入（含非法 UTF-8 经 lossy 转换）都不得 panic。
#![no_main]

use libfuzzer_sys::fuzz_target;
use shift_browser_lib::fuzz_bridge::fuzz_helpers;

fuzz_target!(|data: &[u8]| {
    // 非法 UTF-8 字节经 lossy 替换后同样是解析器现实中会收到的输入
    let s = String::from_utf8_lossy(data);
    let _ = fuzz_helpers::url_decode(&s);
    let _ = fuzz_helpers::kv_value(&s, "token");
    let _ = fuzz_helpers::content_length(&s);
    let _ = fuzz_helpers::is_chunked(&s);

    // 合法 UTF-8 输入再走一遍（覆盖多边界字符场景）
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = fuzz_helpers::url_decode(s);
        let _ = fuzz_helpers::kv_value(s, "token");
    }
});
