#!/usr/bin/env python3
"""
Gaggle 多方RFP谈判一键测试脚本

功能：
1. 启动 Gaggle 服务器
2. 并发启动 3 个 Provider Agent
3. 启动 1 个 Hermes Consumer Agent
4. 执行完整 RFP 谈判流程
5. 打印所有 Provider 的信誉评分
6. 清理资源

使用方式:
  python scripts/multi_party_test.py
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from typing import List, Dict, Optional
import requests
import threading


class MultiPartyTestRunner:
    def __init__(self, server_port: int = 3000, cargo_path: str = "cargo"):
        self.server_port = server_port
        self.server_url = f"localhost:{server_port}"
        self.cargo_path = cargo_path

        self.server_process: Optional[subprocess.Popen] = None
        self.provider_processes: List[subprocess.Popen] = []
        self.hermes_process: Optional[subprocess.Popen] = None

        self.base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

    def log(self, message: str, level: str = "INFO"):
        """打印日志"""
        timestamp = time.strftime("%H:%M:%S")
        print(f"[{timestamp}] [{level}] {message}")

    def check_server_running(self) -> bool:
        """检查服务器是否已在运行"""
        try:
            resp = requests.get(f"http://{self.server_url}/api/v1/providers/search", timeout=2)
            return resp.status_code in [200, 404]  # 404 说明路由存在但没数据
        except requests.exceptions.RequestException:
            return False

    def start_server(self) -> bool:
        """启动 Gaggle 服务器"""
        if self.check_server_running():
            self.log("Server already running, skipping start")
            return True

        self.log("Starting Gaggle server...")

        # 使用 cargo run 启动服务器
        env = os.environ.copy()
        env["RUST_LOG"] = "debug"

        try:
            self.server_process = subprocess.Popen(
                [self.cargo_path, "run"],
                cwd=self.base_dir,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True
            )

            # 等待服务器启动
            for i in range(30):  # 最多等待30秒
                time.sleep(1)
                if self.check_server_running():
                    self.log(f"Server started on port {self.server_port}")
                    return True
                if self.server_process.poll() is not None:
                    self.log("Server process exited unexpectedly", "ERROR")
                    return False

            self.log("Server startup timeout", "ERROR")
            return False

        except FileNotFoundError:
            self.log(f"Cargo not found at {self.cargo_path}", "ERROR")
            return False
        except Exception as e:
            self.log(f"Failed to start server: {e}", "ERROR")
            return False

    def start_providers(self, count: int = 3) -> bool:
        """启动多个 Provider Agent"""
        provider_names = [
            "专业设计服务商",
            "创意工作室",
            "高端设计公司"
        ][:count]

        self.log(f"Starting {count} provider agents...")

        for name in provider_names:
            provider_script = os.path.join(self.base_dir, "scripts", "provider_agent.py")

            if not os.path.exists(provider_script):
                self.log(f"Provider script not found: {provider_script}", "ERROR")
                return False

            try:
                process = subprocess.Popen(
                    [sys.executable, provider_script,
                     "--name", name,
                     "--server", self.server_url],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True
                )
                self.provider_processes.append(process)
                self.log(f"Started provider: {name} (PID={process.pid})")

            except Exception as e:
                self.log(f"Failed to start provider {name}: {e}", "ERROR")
                return False

        # 等待 providers 注册
        time.sleep(3)

        # 验证 providers 已注册
        try:
            resp = requests.get(f"http://{self.server_url}/api/v1/providers/search", timeout=5)
            if resp.status_code == 200:
                providers = resp.json()
                self.log(f"Verified {len(providers)} providers registered")
                if len(providers) < count:
                    self.log(f"Warning: Expected {count} providers, found {len(providers)}")
        except requests.exceptions.RequestException as e:
            self.log(f"Failed to verify providers: {e}", "ERROR")
            return False

        return True

    def start_hermes(self, rfp_name: str = "Logo设计RFP") -> bool:
        """启动 Hermes Consumer Agent"""
        hermes_script = os.path.join(self.base_dir, "scripts", "hermes_consumer.py")

        if not os.path.exists(hermes_script):
            self.log(f"Hermes script not found: {hermes_script}", "ERROR")
            return False

        self.log("Starting Hermes consumer agent...")

        try:
            self.hermes_process = subprocess.Popen(
                [sys.executable, hermes_script,
                 "--server", self.server_url,
                 "--rfp-name", rfp_name],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True
            )
            self.log(f"Started Hermes (PID={self.hermes_process.pid})")
            return True

        except Exception as e:
            self.log(f"Failed to start Hermes: {e}", "ERROR")
            return False

    def wait_for_completion(self, timeout: int = 120) -> bool:
        """等待谈判完成"""
        self.log(f"Waiting for negotiation to complete (timeout={timeout}s)...")

        start_time = time.time()

        while time.time() - start_time < timeout:
            # 检查 Hermes 是否已完成
            if self.hermes_process and self.hermes_process.poll() is not None:
                self.log("Hermes process completed")
                return True

            time.sleep(2)

        self.log("Negotiation timeout", "WARN")
        return False

    def get_reputations(self) -> Dict[str, Dict]:
        """获取所有 Provider 的信誉评分"""
        self.log("Fetching reputation scores...")

        try:
            resp = requests.get(f"http://{self.server_url}/api/v1/providers/search", timeout=5)
            if resp.status_code != 200:
                return {}

            providers = resp.json()
            reputations = {}

            for provider in providers:
                agent_id = provider.get("id")
                profile = provider.get("profile", {})

                if not agent_id:
                    continue

                # 获取信誉详情
                resp_rep = requests.get(
                    f"http://{self.server_url}/api/v1/agents/{agent_id}/reputation",
                    timeout=5
                )

                if resp_rep.status_code == 200:
                    rep_data = resp_rep.json()
                    summary = rep_data.get("summary", {})
                    reputations[agent_id] = {
                        "name": profile.get("display_name", "Unknown"),
                        "score": summary.get("reputation_score", 0),
                        "total_negotiations": summary.get("total_negotiations", 0),
                        "success_rate": summary.get("fulfillment_rate", 0),
                        "avg_rating": summary.get("avg_rating")
                    }

            return reputations

        except requests.exceptions.RequestException as e:
            self.log(f"Failed to fetch reputations: {e}", "ERROR")
            return {}

    def print_reputations(self, reputations: Dict[str, Dict]):
        """打印信誉评分"""
        print("\n" + "=" * 60)
        print("PROVIDER REPUTATION SCORES")
        print("=" * 60)

        if not reputations:
            print("No reputation data available")
            return

        for agent_id, rep in reputations.items():
            print(f"\n{rep['name']} ({agent_id[:8]}...)")
            print(f"  Reputation Score: {rep['score']:.2f}")
            print(f"  Total Negotiations: {rep['total_negotiations']}")
            print(f"  Success Rate: {rep['success_rate'] * 100:.1f}%")
            if rep['avg_rating']:
                print(f"  Average Rating: {rep['avg_rating']:.2f}/5")

        print("\n" + "=" * 60)

    def cleanup(self):
        """清理资源"""
        self.log("Cleaning up...")

        # 终止 Hermes
        if self.hermes_process:
            try:
                self.hermes_process.terminate()
                self.hermes_process.wait(timeout=5)
                self.log("Hermes process terminated")
            except subprocess.TimeoutExpired:
                self.hermes_process.kill()
            except Exception as e:
                self.log(f"Error terminating Hermes: {e}", "WARN")

        # 终止 Providers
        for i, proc in enumerate(self.provider_processes):
            try:
                proc.terminate()
                proc.wait(timeout=5)
                self.log(f"Provider {i+1} terminated")
            except subprocess.TimeoutExpired:
                proc.kill()
            except Exception as e:
                self.log(f"Error terminating provider {i+1}: {e}", "WARN")

        # 终止服务器
        if self.server_process:
            try:
                self.server_process.terminate()
                self.server_process.wait(timeout=10)
                self.log("Server process terminated")
            except subprocess.TimeoutExpired:
                self.server_process.kill()
            except Exception as e:
                self.log(f"Error terminating server: {e}", "WARN")

        self.log("Cleanup complete")

    def run(self, provider_count: int = 3, rfp_name: str = "Logo设计RFP") -> bool:
        """运行完整测试"""
        print("\n" + "=" * 60)
        print("GAGGLE MULTI-PARTY RFP TEST")
        print("=" * 60)

        try:
            # 1. 启动服务器
            if not self.start_server():
                return False

            # 2. 启动 Providers
            if not self.start_providers(provider_count):
                self.cleanup()
                return False

            # 3. 启动 Hermes
            if not self.start_hermes(rfp_name):
                self.cleanup()
                return False

            # 4. 等待完成
            if not self.wait_for_completion():
                self.log("Test did not complete in time", "WARN")

            # 5. 获取并打印信誉评分
            reputations = self.get_reputations()
            self.print_reputations(reputations)

            print("\n" + "=" * 60)
            print("TEST COMPLETED SUCCESSFULLY")
            print("=" * 60 + "\n")

            return True

        except KeyboardInterrupt:
            self.log("\nTest interrupted by user", "WARN")
            return False
        except Exception as e:
            self.log(f"Test failed with error: {e}", "ERROR")
            return False
        finally:
            self.cleanup()


def main():
    parser = argparse.ArgumentParser(description="Gaggle 多方RFP谈判一键测试")
    parser.add_argument("--server-port", type=int, default=3000, help="服务器端口")
    parser.add_argument("--provider-count", type=int, default=3, help="Provider 数量")
    parser.add_argument("--rfp-name", default="Logo设计RFP", help="RFP 名称")
    parser.add_argument("--cargo", default="cargo", help="Cargo 可执行文件路径")
    parser.add_argument("--skip-server", action="store_true", help="跳过服务器启动（假设已运行）")

    args = parser.parse_args()

    runner = MultiPartyTestRunner(
        server_port=args.server_port,
        cargo_path=args.cargo
    )

    # 如果设置了 --skip-server，跳过服务器启动
    if args.skip_server:
        runner.log("Skipping server start (--skip-server flag set)")

        # 临时禁用 start_server
        original_start = runner.start_server
        runner.start_server = lambda: True

    success = runner.run(
        provider_count=args.provider_count,
        rfp_name=args.rfp_name
    )

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
