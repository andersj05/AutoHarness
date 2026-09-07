import unittest

from ci_scope import classify


class ScopeTests(unittest.TestCase):
    def test_docs_and_tooling_do_not_compile_desktop(self):
        self.assertFalse(any(classify(["docs/release/check.md", "scripts/gui_webdriver.py"]).values()))

    def test_frontend_does_not_repeat_platform_runtime_tests(self):
        self.assertEqual(classify(["apps/gui/src/App.tsx"]),
                         {"rust": False, "frontend": True, "benchmarks": False})

    def test_shared_contract_validates_both_consumers(self):
        self.assertTrue(all(classify(["crates/autoharness-client/src/lib.rs"]).values()))

    def test_runtime_validates_benchmark_consumer(self):
        self.assertEqual(classify(["crates/autoharness-store/src/lib.rs"]),
                         {"rust": True, "frontend": False, "benchmarks": True})

    def test_unknown_and_workflow_changes_fail_towards_full_coverage(self):
        for path in ("Cargo.lock", ".github/workflows/ci.yml", "new-runtime/build.config"):
            with self.subTest(path=path):
                self.assertTrue(all(classify([path]).values()))

    def test_mixed_changes_union_coverage(self):
        self.assertTrue(all(classify(["apps/gui/src/App.tsx", "benchmarks/main.rs",
                                      "crates/autoharness-app/build.rs"]).values()))


if __name__ == "__main__":
    unittest.main()
