import json
import os
import sys

path = os.environ["CONGMIAO_SMOKE"]
if not os.path.exists(path):
    print("没有写出冒烟报告", path)
    sys.exit(1)
report = json.load(open(path, encoding="utf-8"))
print(json.dumps(report, ensure_ascii=False, indent=2))
if not report.get("ok"):
    sys.exit(1)
