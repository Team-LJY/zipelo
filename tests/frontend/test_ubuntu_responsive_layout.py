"""Ubuntu 목록 공간과 화면 연결의 정적 회귀를 검사함."""

from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

PROJECT = Path(__file__).resolve().parents[2]
UI = "{https://github.com/avaloniaui}"
NAME = "{http://schemas.microsoft.com/winfx/2006/xaml}Name"


class UbuntuResponsiveLayoutTests(unittest.TestCase):
    def setUp(self):
        self.window = ET.parse(PROJECT / "src/Zipelo.Ubuntu/MainWindow.axaml").getroot()
        self.controls = {node.get(NAME): node for node in self.window.iter() if node.get(NAME)}
        self.parents = {child: parent for parent in self.window.iter() for child in parent}

    def test_default_and_minimum_window_sizes_are_preserved(self):
        for key, value in {"Width": "960", "Height": "720", "MinWidth": "640", "MinHeight": "500"}.items():
            self.assertEqual(self.window.get(key), value)
        rows = self.window.find(UI + "Grid/" + UI + "Grid.RowDefinitions")
        self.assertTrue(rows[0].get("Height").endswith("*"))
        self.assertTrue(rows[1].get("Height").endswith("*"))
        self.assertGreaterEqual(float(rows[1].get("MinHeight")), 168)
        self.assertEqual(rows[2].get("Height"), "Auto")

    def test_form_and_status_have_separate_bounded_viewports(self):
        form = self.window.find(UI + "Grid/" + UI + "ScrollViewer")
        self.assertEqual(form.get("Grid.Row"), "0")
        for name in ["OpenButton", "CreateButton", "RetryCreateButton", "ExtractButton",
                     "ArchiveTitle", "Availability", "CreationName", "ExtractionName",
                     "CreationSelection", "ExtractionSelection"]:
            self.assertIn(self.controls[name], list(form.iter()))
        status_scroll = self.parents[self.controls["Status"]]
        self.assertEqual(status_scroll.tag, UI + "ScrollViewer")
        self.assertLessEqual(float(status_scroll.get("MaxHeight")), 96)
        for viewport in [form, status_scroll]:
            self.assertEqual(viewport.get("VerticalScrollBarVisibility"), "Auto")
            self.assertEqual(viewport.get("HorizontalScrollBarVisibility"), "Disabled")
            self.assertEqual(viewport.get("Focusable"), "True")
            self.assertTrue(viewport.get("AutomationProperties.Name"))
        self.assertNotIn(self.controls["StopButton"], list(form.iter()))
        self.assertNotIn(self.controls["StopButton"], list(status_scroll.iter()))

    def test_empty_state_and_long_text_wrap_without_covering_selection(self):
        empty = self.controls["EmptyState"]
        self.assertEqual(empty.get("TextWrapping"), "Wrap")
        self.assertEqual(empty.get("IsHitTestVisible"), "False")
        self.assertEqual(empty.get("HorizontalAlignment"), "Stretch")
        self.assertIs(self.parents[empty], self.parents[self.controls["Files"]])
        self.assertEqual(self.parents[empty].get("Grid.Row"), "1")
        for name in ["ArchiveTitle", "Availability", "CreationSelection", "ExtractionSelection", "Status"]:
            self.assertEqual(self.controls[name].get("TextWrapping"), "Wrap")
        template = self.controls["Files"].find(".//" + UI + "DataTemplate/" + UI + "Grid")
        self.assertEqual(template.get("ColumnDefinitions"), "3*,*,*")
        self.assertEqual([node.get("Text") for node in template],
                         ["{Binding Name}", "{Binding Size}", "{Binding Verification}"])
        self.assertTrue(all(node.get("TextWrapping") == "Wrap" for node in template))

    def test_stretch_is_scoped_to_listbox_item_containers(self):
        listing = self.controls["Files"]
        self.assertNotIn("HorizontalContentAlignment", listing.attrib)
        self.assertEqual(listing.get("ScrollViewer.HorizontalScrollBarVisibility"), "Disabled")
        styles = listing.find(UI + "ListBox.Styles")
        self.assertIsNotNone(styles)
        self.assertEqual(len(styles), 1)
        self.assertEqual(styles[0].get("Selector"), "ListBoxItem")
        setters = list(styles[0])
        self.assertEqual(len(setters), 1)
        self.assertEqual(setters[0].get("Property"), "HorizontalContentAlignment")
        self.assertEqual(setters[0].get("Value"), "Stretch")
        self.assertIsNone(listing.find(UI + "ListBox.ItemContainerTheme"))

    def test_commands_initial_gates_and_accessible_names_are_preserved(self):
        clicks = {"OpenButton": "OpenArchive", "CreateButton": "CreateArchive",
                  "RetryCreateButton": "RetryCreation", "ExtractButton": "ExtractArchive",
                  "StopButton": "StopJob"}
        for name, handler in clicks.items():
            self.assertEqual(self.controls[name].get("Click"), handler)
            if name != "OpenButton":
                self.assertEqual(self.controls[name].get("IsEnabled"), "False")
            self.assertNotEqual(self.controls[name].get("IsVisible"), "False")
        self.assertEqual(self.window.get("Closing"), "OnClosing")
        for name in ["CreationName", "ExtractionName", "Files", "Status", "Progress"]:
            self.assertTrue(self.controls[name].get("AutomationProperties.Name"))
        self.assertEqual(self.controls["Progress"].get("IsVisible"), "False")
        self.assertEqual(set(self.controls), {
            "OpenButton", "CreateButton", "RetryCreateButton", "ExtractButton", "StopButton",
            "ArchiveTitle", "Availability", "CreationName", "CreationSelection",
            "ExtractionName", "ExtractionSelection", "Files", "EmptyState", "Status", "Progress",
        })


if __name__ == "__main__":
    unittest.main()
