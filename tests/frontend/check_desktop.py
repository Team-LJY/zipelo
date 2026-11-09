from pathlib import Path
import re
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
DESKTOP = ROOT / "src" / "Zipelo.Desktop"
XAML_NAME = "{http://schemas.microsoft.com/winfx/2006/xaml}Name"


class DesktopStructureTests(unittest.TestCase):
    def test_markup_events_have_code_handlers(self):
        for path in DESKTOP.glob("*.xaml"):
            tree = ET.parse(path)
            code = Path(str(path) + ".cs").read_text()
            for element in tree.iter():
                for event in ("Click", "Closing"):
                    if event in element.attrib:
                        handler = element.attrib[event]
                        self.assertRegex(code, rf"\b{re.escape(handler)}\s*\(")

    def test_named_controls_used_by_code_exist(self):
        tree = ET.parse(DESKTOP / "MainWindow.xaml")
        controls = {
            element.attrib[XAML_NAME]: element
            for element in tree.iter()
            if XAML_NAME in element.attrib
        }
        for name in (
            "OpenButton", "CreateButton", "ExtractButton", "ArchiveTitle",
            "Availability", "Files", "EmptyState", "Status", "Progress", "StopButton",
        ):
            self.assertIn(name, controls)
        self.assertEqual(controls["Files"].attrib["IsReadOnly"], "True")
        self.assertEqual(controls["StopButton"].attrib["IsEnabled"], "False")
        self.assertEqual(controls["Status"].attrib["AutomationProperties.LiveSetting"], "Polite")

    def test_password_has_no_plaintext_binding_or_persistence(self):
        tree = ET.parse(DESKTOP / "PasswordWindow.xaml")
        secret = next(element for element in tree.iter() if element.attrib.get(XAML_NAME) == "Secret")
        self.assertTrue(secret.tag.endswith("PasswordBox"))
        for path in DESKTOP.glob("*.cs"):
            code = path.read_text()
            self.assertNotRegex(code, r"\bSecret\.Password\b")
            self.assertNotRegex(code, r"(?:Console|Debug|Trace)\.Write")
            self.assertNotIn("Process.Start", code)
            self.assertNotIn("SetEnvironmentVariable", code)
        code = (DESKTOP / "PasswordWindow.xaml.cs").read_text()
        self.assertIn("Secret.Clear()", code)
        self.assertIn("SecurePassword", code)

    def test_startup_explicitly_has_no_engine(self):
        code = (DESKTOP / "App.xaml.cs").read_text()
        self.assertIn("new UnavailableWorkspace()", code)
        port = (DESKTOP / "WorkspacePort.cs").read_text()
        self.assertIn("IsAvailable => false", port)
        self.assertIn("throw new NotSupportedException", port)

    def test_download_placeholder_has_no_external_or_active_content(self):
        tree = ET.parse(ROOT / "assets" / "download-pending.svg")
        for element in tree.iter():
            self.assertNotIn(element.tag.split("}")[-1], ("script", "foreignObject", "image", "a"))
            for attribute in element.attrib:
                self.assertFalse(attribute.startswith("on"))
                self.assertNotIn("href", attribute)
        self.assertIn("준비 중", "".join(tree.getroot().itertext()))

    def test_publish_is_windows_x64_without_unsafe_trimming(self):
        tree = ET.parse(DESKTOP / "Properties" / "PublishProfiles" / "WindowsX64.pubxml")
        self.assertEqual(tree.findtext(".//RuntimeIdentifier"), "win-x64")
        self.assertEqual(tree.findtext(".//PublishTrimmed"), "false")
        ET.parse(DESKTOP / "Zipelo.Desktop.csproj")
        manifest = ET.parse(DESKTOP / "app.manifest")
        privilege = next(element for element in manifest.iter() if element.tag.endswith("requestedExecutionLevel"))
        self.assertEqual(privilege.attrib["level"], "asInvoker")


if __name__ == "__main__":
    unittest.main()
