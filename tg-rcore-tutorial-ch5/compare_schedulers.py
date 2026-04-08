#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
调度器性能对比工具
对所有5种调度器分别编译、运行、收集性能数据并生成对比表
"""

import subprocess
import re
import sys
import os
from pathlib import Path
from typing import Dict, List, Tuple
from collections import defaultdict

# 设置输出编码为UTF-8（用于Windows支持）
if sys.platform == 'win32':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

class SchedulerBenchmark:
    """调度器性能对比工具"""
    
    SCHEDULERS = [
        ("FCFS", "scheduler-fcfs"),
        ("SJF", "scheduler-sjf"),
        ("RR", "scheduler-rr"),
        ("MLFQ", "scheduler-mlfq"),
        ("CFS", "scheduler-cfs"),
    ]
    
    def __init__(self, work_dir: str):
        self.work_dir = Path(work_dir)
        self.results: Dict[str, Dict] = {}
        
    def compile_scheduler(self, name: str, feature: str) -> Tuple[bool, str]:
        """编译指定调度器并返回编译状态和日志"""
        print(f"\n{'='*70}")
        print(f"编译调度器: {name} (feature: {feature})")
        print('='*70)
        
        # 编译前清理所有遗留QEMU进程
        self.kill_qemu_processes()
        
        # 清理cargo缓存以避免feature切换导致的问题
        import time
        print("清理编译缓存...")
        clean_result = subprocess.run(
            "cargo clean",
            cwd=str(self.work_dir),
            capture_output=True,
            text=True,
            shell=True
        )
        time.sleep(1)  # 等待清理完成
        print(f"{clean_result.stdout}")
        
        # 使用shell命令以便正确处理PATH
        cmd = f"cargo build --release --no-default-features --features={feature}"
        
        try:
            result = subprocess.run(
                cmd,
                cwd=str(self.work_dir),
                capture_output=True,
                text=True,
                timeout=300,
                encoding='utf-8',
                errors='ignore',
                shell=True  # 使用shell=True让PATH正确解析
            )
            
            if result.returncode == 0:
                print(f"✅ {name} 编译成功")
                return True, ""
            else:
                print(f"❌ {name} 编译失败")
                return False, result.stderr
                
        except subprocess.TimeoutExpired:
            print(f"⏱️  {name} 编译超时")
            return False, "Timeout"
        except Exception as e:
            print(f"❌ {name} 编译错误: {e}")
            return False, str(e)
    
    def kill_qemu_processes(self):
        """杀死所有遗留的QEMU进程"""
        try:
            if sys.platform == 'win32':
                # Windows: 尝试多种方式杀死进程
                os.system('taskkill /F /IM qemu-system-riscv64.exe 2>nul')
            else:
                # Linux/WSL: 使用多种方式确保彻底清理
                os.system('pkill -9 qemu-system-riscv64 2>/dev/null')
                os.system('killall -9 qemu-system-riscv64 2>/dev/null')
                os.system('pkill -9 -f qemu-system-riscv64 2>/dev/null')
                # 等待进程完全退出
                import time
                time.sleep(0.5)
        except:
            pass
    
    def run_scheduler_test(self, name: str) -> Tuple[bool, Dict]:
        """运行调度器测试并收集性能数据"""
        print(f"\n运行测试: {name}")
        print('-'*70)
        
        # 清理遗留的QEMU进程
        self.kill_qemu_processes()
        
        # 使用shell命令以便正确处理PATH
        cmd = "cargo run --release"
        
        env = os.environ.copy()
        env["CHAPTER"] = "5"
        
        try:
            # 使用 Popen 而不是 run，以便在超时时强制杀死进程
            from subprocess import PIPE, Popen, TimeoutExpired
            
            process = Popen(
                cmd,
                cwd=str(self.work_dir),
                stdout=PIPE,
                stderr=PIPE,
                text=True,
                env=env,
                bufsize=1,
                encoding='utf-8',
                errors='ignore',
                shell=True  # 使用shell=True让PATH正确解析
            )
            
            try:
                # 对于所有调度器都使用更长的超时（300秒 = 5分钟）
                timeout = 300
                stdout, stderr = process.communicate(timeout=timeout)
                output = stdout + stderr
                
                print(f"✅ {name} 运行成功")
                metrics = self.parse_metrics(output)
                task_count = metrics.get('task_count', 0)
                total_time = metrics.get('total_time', 0)
                
                print(f"   任务数: {task_count}")
                print(f"   总耗时: {total_time} ms")
                
                # 运行成功后立即强制清理进程
                self.kill_qemu_processes()
                import time
                time.sleep(1)  # 等待确保清理完全
                return True, metrics
                
            except TimeoutExpired:
                # 超时时杀死进程
                print(f"⏱️  {name} 运行超时（超过{timeout}秒），强制终止...")
                process.kill()
                try:
                    stdout, stderr = process.communicate(timeout=5)
                except:
                    try:
                        process.terminate()
                    except:
                        pass
                
                # 强制清理所有QEMU进程（多次确保）
                import time
                self.kill_qemu_processes()
                time.sleep(1)
                self.kill_qemu_processes()
                time.sleep(0.5)
                self.kill_qemu_processes()
                return False, {}
                
        except Exception as e:
            print(f"❌ {name} 运行错误: {e}")
            self.kill_qemu_processes()
            return False, {}
    
    def parse_metrics(self, output: str) -> Dict:
        """从输出解析性能指标"""
        metrics = {
            'task_count': 0,
            'total_time': 0,
            'tasks': [],
            'workloads': defaultdict(list),
            'completion_order': [],  # 任务完成顺序（用于推断调度策略）
        }
        
        # 提取全局时间
        global_match = re.search(r'\[WORKLOAD_END\]\s+TIME=(\d+)\s+TOTAL=(\d+)', output)
        if global_match:
            metrics['total_time'] = int(global_match.group(2))
        
        # 提取任务信息
        task_pattern = r'\[TASK_START\]\s+PID=(\d+)\s+TYPE=(\w+)\s+ID=(\d+)\s+TIME=(\d+)'
        task_end_pattern = r'\[TASK_END\]\s+PID=(\d+)\s+TYPE=(\w+)\s+ID=(\d+)\s+TIME=(\d+)\s+DURATION=(\d+)'
        
        tasks = {}
        task_list_by_completion = []  # 按完成时间排序的任务列表
        
        for match in re.finditer(task_pattern, output):
            pid, task_type, task_id, start_time = match.groups()
            tasks[pid] = {
                'pid': int(pid),
                'type': task_type,
                'id': int(task_id),
                'start_time': int(start_time),
                'end_time': 0,
                'duration': 0,
            }
        
        for match in re.finditer(task_end_pattern, output):
            pid, task_type, task_id, end_time, duration = match.groups()
            if pid in tasks:
                tasks[pid]['end_time'] = int(end_time)
                tasks[pid]['duration'] = int(duration)
            else:
                # 如果没有TASK_START记录，创建一个
                tasks[pid] = {
                    'pid': int(pid),
                    'type': task_type,
                    'id': int(task_id),
                    'start_time': int(end_time) - int(duration),
                    'end_time': int(end_time),
                    'duration': int(duration),
                }
        
        # 按工作负载分类统计
        task_list = list(tasks.values())
        
        # 生成任务完成顺序（按end_time排序）
        task_list_by_completion = sorted(
            [t for t in task_list if t['end_time'] > 0],
            key=lambda x: x['end_time']
        )
        
        # 构造completion_order数据
        for task in task_list_by_completion:
            metrics['completion_order'].append({
                'pid': task['pid'],
                'type': task['type'],
                'id': task['id'],
                'end_time': task['end_time'],
                'duration': task['duration'],
            })
        
        metrics['task_count'] = len(task_list)
        metrics['tasks'] = task_list
        
        # 计算工作负载统计
        for task in task_list:
            workload_type = task['type']
            metrics['workloads'][workload_type].append(task['duration'])
        
        # 计算每个工作负载的平均时间
        workload_stats = {}
        for wl_type, durations in metrics['workloads'].items():
            if durations:
                avg_duration = sum(durations) / len(durations)
                workload_stats[wl_type] = {
                    'count': len(durations),
                    'avg': avg_duration,
                    'min': min(durations),
                    'max': max(durations),
                }
        metrics['workload_stats'] = workload_stats
        
        return metrics
    
    def run_all_benchmarks(self) -> bool:
        """运行所有调度器的基准测试"""
        print("\n" + "╔"+"="*68+"╗")
        print("║" + " "*18 + "调度器性能对比测试" + " "*24 + "║")
        print("╚"+"="*68+"╝")
        
        try:
            for name, feature in self.SCHEDULERS:
                # 编译
                success, _ = self.compile_scheduler(name, feature)
                if not success:
                    self.results[name] = {
                        "compile": "FAIL",
                        "metrics": {}
                    }
                    continue
                
                # 运行测试
                run_success, metrics = self.run_scheduler_test(name)
                self.results[name] = {
                    "compile": "PASS",
                    "run": "PASS" if run_success else "FAIL",
                    "metrics": metrics if run_success else {}
                }
            
            # 生成对比表
            self.print_comparison_table()
            return True
        finally:
            # 确保清理遗留进程
            self.kill_qemu_processes()
    
    def print_comparison_table(self):
        """生成并打印性能对比表"""
        print("\n" + "="*80)
        print("性能对比表")
        print("="*80)
        
        # 表头
        print(f"{'调度器':<10} {'编译':<6} {'运行':<6} {'总耗时(ms)':<12} {'任务数':<8} {'CPU平均(ms)':<14} {'IO平均(ms)':<14} {'交互平均(ms)':<14}")
        print("-"*106)
        
        # 数据行
        for name, result in self.results.items():
            compile_status = "✅" if result['compile'] == "PASS" else "❌"
            run_status = "✅" if result.get('run') == "PASS" else "❌"
            
            metrics = result.get('metrics', {})
            total_time = metrics.get('total_time', 0)
            task_count = metrics.get('task_count', 0)
            
            wl_stats = metrics.get('workload_stats', {})
            cpu_avg = wl_stats.get('CPU', {}).get('avg', 0)
            io_avg = wl_stats.get('IO', {}).get('avg', 0)
            inter_avg = wl_stats.get('INTER', {}).get('avg', 0)
            
            print(f"{name:<10} {compile_status:<6} {run_status:<6} {total_time:<12} {task_count:<8} {cpu_avg:<14.2f} {io_avg:<14.2f} {inter_avg:<14.2f}")
        
        # 详细统计
        print("\n" + "="*80)
        print("详细统计信息与调度顺序分析")
        print("="*80)
        
        for name, result in self.results.items():
            if result['compile'] != "PASS":
                continue
                
            metrics = result.get('metrics', {})
            wl_stats = metrics.get('workload_stats', {})
            
            print(f"\n【{name} 调度器】")
            print(f"  总耗时: {metrics.get('total_time', 0)} ms")
            print(f"  任务总数: {metrics.get('task_count', 0)}")
            
            for wl_type, stats in wl_stats.items():
                print(f"  {wl_type} 工作负载:")
                print(f"    - 任务数: {stats['count']}")
                print(f"    - 平均耗时: {stats['avg']:.2f} ms")
                print(f"    - 最小耗时: {stats['min']:.2f} ms")
                print(f"    - 最大耗时: {stats['max']:.2f} ms")
            
            # 显示任务完成顺序（关键用于推断调度策略）
            completion_order = metrics.get('completion_order', [])
            if completion_order:
                print(f"\n  任务完成顺序（按执行完成时间序列）:")
                print(f"  {'序号':<5} {'PID':<5} {'类型':<8} {'ID':<4} {'完成时间':<12} {'耗时':<10}")
                print(f"  {'-'*50}")
                
                for idx, task in enumerate(completion_order, 1):
                    pid = task['pid']
                    task_type = task['type']
                    task_id = task['id']
                    end_time = task['end_time']
                    duration = task['duration']
                    
                    print(f"  {idx:<5} {pid:<5} {task_type:<8} {task_id:<4} {end_time:<12} {duration:<10}")
                
                # 推断调度策略的诊断信息
                print(f"\n  📊 调度策略诊断:")
                print(f"     - 任务执行顺序反映了调度器的决策")
                print(f"     - FCFS: 任务按生成顺序完成（ID逐个递增）")
                print(f"     - SJF: 短任务先完成（较小的耗时先出现）")
                print(f"     - RR/MLFQ/CFS: 混合顺序，体现时间片或优先级机制")
        
        # 对比总结
        print("\n" + "="*80)
        print("性能总结（按总耗时排序）")
        print("="*80)
        
        sorted_results = sorted(
            [(name, result) for name, result in self.results.items()
             if result['compile'] == "PASS" and result.get('run') == "PASS"],
            key=lambda x: x[1].get('metrics', {}).get('total_time', float('inf'))
        )
        
        print(f"{'排名':<6} {'调度器':<10} {'总耗时(ms)':<15} {'改进':<10}")
        print("-"*50)
        
        if sorted_results:
            best_time = sorted_results[0][1].get('metrics', {}).get('total_time', 1)
            
            for rank, (name, result) in enumerate(sorted_results, 1):
                total_time = result.get('metrics', {}).get('total_time', 0)
                improvement = ((total_time - best_time) / best_time * 100) if best_time > 0 else 0
                
                if rank == 1:
                    print(f"{rank:<6} {name:<10} {total_time:<15} {'🏆 最优':<10}")
                else:
                    print(f"{rank:<6} {name:<10} {total_time:<15} {improvement:+.1f}%")
        
        # 并发对比提示
        print("\n" + "="*80)
        print("调度器对比要点")
        print("="*80)
        print("""
  运行性原理分析：
  
  FCFS（先来先服务）：
    - 优点：简单，低开销
    - 缺点：长任务阻挡短任务，交互性差
    - 特征：完成顺序 = 生成顺序
  
  SJF（最短作业优先）：
    - 优点：总体吞吐量最高，短任务先运行
    - 缺点：需要预知任务时长，饥荒问题
    - 特征：短任务完成时间早于长任务
  
  RR（轮转调度）：
    - 优点：公平，交互性好
    - 缺点：频繁切换，缓存局部性差
    - 特征：CPU与IO任务交替运行
  
  MLFQ（多级反馈队列）：
    - 优点：自适应，无需预知时长，兼顾公平和性能
    - 缺点：参数复杂
    - 特征：行为表现介于SJF和RR之间
  
  CFS（完全公平调度）：
    - 优点：公平，良好的交互性
    - 缺点：比RR复杂
    - 特征：虚拟运行时均衡，IO任务得到补偿
        """)


def main():
    work_dir = Path(__file__).parent if Path(__file__).parent.is_dir() else Path.cwd()
    print(f"DEBUG: Working directory: {work_dir}", flush=True)
    print(f"DEBUG: Cargo.toml exists: {(work_dir / 'Cargo.toml').exists()}", flush=True)
    
    if not (work_dir / "Cargo.toml").exists():
        print("❌ 错误: 未找到 Cargo.toml")
        print("   请从 tg-rcore-tutorial-ch5 目录运行此脚本")
        sys.exit(1)
    
    benchmark = SchedulerBenchmark(str(work_dir))
    benchmark.run_all_benchmarks()

if __name__ == "__main__":
    main()
