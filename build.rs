fn main() {
    // Compiles Outpost's interface icons into the program, so GTK can recolor them
    // for light and dark mode on every GTK version.
    glib_build_tools::compile_resources(
        &["data/icons/fallback"],
        "data/icons/fallback/icons.gresource.xml",
        "icons.gresource",
    );
}
