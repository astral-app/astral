use astral_core::diagnostics::metrickit::{crash_stack, disk_write_stack, hang_stack};
use serde_json::json;

fn diagnostic(binary: &str) -> serde_json::Value {
    json!({
        "diagnosticMetaData": {
            "signal": "SIGSEGV",
            "exceptionType": 1,
            "exceptionCode": 0,
            "terminationReason": "Namespace SIGNAL",
            "hangDuration": "2 sec",
            "writesCaused": "1 GB"
        },
        "callStackTree": {
            "callStacks": [
                { "callStackRootFrames": [{ "address": 1, "binaryName": "Other" }] },
                {
                    "threadAttributed": true,
                    "callStackRootFrames": [{
                        "address": 4096,
                        "binaryName": binary,
                        "offsetIntoBinaryTextSegment": 255,
                        "subFrames": [{ "address": 8192 }]
                    }]
                }
            ]
        }
    })
}

#[test]
fn formats_the_attributed_thread() {
    let text = crash_stack(&diagnostic("Discord")).expect("stack");
    assert_eq!(
        text,
        "Signal SIGSEGV\nException 1 code 0\nTermination Namespace SIGNAL\n#0 0x1000 Discord + 0xff\n#1 0x2000\n"
    );
    assert!(
        hang_stack(&diagnostic("Discord"))
            .expect("hang")
            .starts_with("Hang 2 sec\n#0 0x1000")
    );
    assert!(
        disk_write_stack(&diagnostic("Discord"))
            .expect("disk")
            .starts_with("Disk writes 1 GB\n")
    );
}

#[test]
fn rejects_shapes_without_frames() {
    assert!(crash_stack(&json!({})).is_none());
    assert!(crash_stack(&json!({"callStackTree": {"callStacks": []}})).is_none());
}
