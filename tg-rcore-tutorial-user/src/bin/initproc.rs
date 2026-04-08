#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::{exec, fork, wait, println};

// 教学目标：
// 作为用户态“第一个进程”入口，按章节配置启动对应测试集或 shell。

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    println!("[initproc] starting...");
    if fork() == 0 {
        // 子进程执行实际目标程序，父进程负责兜底回收孤儿退出。
        let target = match option_env!("CHAPTER").unwrap_or("0") {
            "5" => "ch5_usertest",
            "6" => "ch6_usertest",
            "8" => "ch8_usertest",
            "-5" => "ch5b_usertest",
            "-6" => "ch6b_usertest",
            "-7" => "ch7b_usertest",
            "-8" => "ch8b_usertest",
            _ => "doom", // 默认运行 doom demo
        };
        println!("[initproc] child will exec: {}", target);
        exec(target);
    } else {
        println!("[initproc] parent waiting for child...");
        loop {
            let mut exit_code: i32 = 0;
            let pid = wait(&mut exit_code);
            if pid == -1 {
                println!("[initproc] no more children, exiting...");
                break;
            }
            println!("[initproc] child {} exited with code {}", pid, exit_code);
        }
    }
    0
}
