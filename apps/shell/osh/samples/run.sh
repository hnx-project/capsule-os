# 自动测试脚本

echo [Step 1] Creating temporary directories...
mkdir -p test_run_dir/sub_dir

echo [Step 2] Testing touch...
touch test_run_dir/sub_dir/hello.txt

echo [Step 3] Checking directory contents using ls...
ls -la test_run_dir/sub_dir

echo [Step 4] Reading created file...
cat test_run_dir/sub_dir/hello.txt

echo [Step 5] Script Execution Completed Successfully!
