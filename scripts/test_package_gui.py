import unittest

from package_gui import signing_config


class SigningTests(unittest.TestCase):
    def test_missing_platform_credentials_fail_closed(self):
        for platform in ("win32", "darwin", "linux"):
            with self.subTest(platform=platform), self.assertRaises(ValueError):
                signing_config(platform, {})

    def test_windows_requires_thumbprint_and_secure_timestamp(self):
        environment = {"AUTOHARNESS_WINDOWS_CERTIFICATE_THUMBPRINT": "a" * 40,
                       "AUTOHARNESS_WINDOWS_TIMESTAMP_URL": "https://timestamp.example.test"}
        self.assertEqual(signing_config("win32", environment)["bundle"]["windows"]["digestAlgorithm"], "sha256")
        environment["AUTOHARNESS_WINDOWS_TIMESTAMP_URL"] = "http://timestamp.example.test"
        with self.assertRaises(ValueError):
            signing_config("win32", environment)

    def test_macos_rejects_adhoc_signing_and_never_serializes_password(self):
        environment = dict(APPLE_SIGNING_IDENTITY="-", APPLE_ID="fixture", APPLE_PASSWORD="sentinel", APPLE_TEAM_ID="fixture")
        with self.assertRaises(ValueError):
            signing_config("darwin", environment)
        environment["APPLE_SIGNING_IDENTITY"] = "Developer ID Application: Fixture"
        self.assertNotIn("sentinel", str(signing_config("darwin", environment)))


if __name__ == "__main__":
    unittest.main()
