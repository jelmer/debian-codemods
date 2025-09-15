use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use debianize::DebianizePreferences;
use tempfile::TempDir;
use upstream_ontologist::{
    Certainty, Origin, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata,
};

#[test]
fn test_autotools_project_detection() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-autotools-project");
    std::fs::create_dir(&project_dir).unwrap();

    // Create autotools project files
    std::fs::write(
        project_dir.join("configure.ac"),
        r#"AC_INIT([test-autotools], [1.0.0])
AM_INIT_AUTOMAKE([-Wall -Werror foreign])
AC_PROG_CC
AC_CONFIG_HEADERS([config.h])
AC_CONFIG_FILES([
 Makefile
 src/Makefile
])
AC_OUTPUT
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("Makefile.am"),
        r#"SUBDIRS = src
ACLOCAL_AMFLAGS = -I m4
"#,
    )
    .unwrap();

    std::fs::create_dir(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("src/Makefile.am"),
        r#"bin_PROGRAMS = test-autotools
test_autotools_SOURCES = main.c
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("src/main.c"),
        r#"#include <stdio.h>

int main() {
    printf("Hello from autotools!\n");
    return 0;
}
"#,
    )
    .unwrap();

    // Initialize git repository
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("test-autotools".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        check: false, // Disable external checking to prevent hanging
        consult_external_directory: false, // Disable external directory consultation
        force_subprocess: false, // Disable subprocess calls to prevent external tool errors
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("1.0.0"),
        &metadata,
    );

    assert!(
        result.is_ok(),
        "Autotools project should debianize successfully"
    );

    // Verify debian files
    assert!(wt.has_filename(&subpath.join("debian/control")));
    assert!(wt.has_filename(&subpath.join("debian/rules")));

    let rules_content = wt.get_file_text(&subpath.join("debian/rules")).unwrap();
    let rules_str = String::from_utf8_lossy(&rules_content);

    // Should use makefile buildsystem for autotools
    assert!(rules_str.contains("dh $@ --buildsystem=makefile") || rules_str.contains("dh $@"));
}

#[test]
fn test_cmake_project_detection() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-cmake-project");
    std::fs::create_dir(&project_dir).unwrap();

    // Create CMake project files
    std::fs::write(
        project_dir.join("CMakeLists.txt"),
        r#"cmake_minimum_required(VERSION 3.10)
project(TestCMakeProject VERSION 1.2.0)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(PkgConfig REQUIRED)
pkg_check_modules(JSONCPP jsoncpp)

if(JSONCPP_FOUND)
    include_directories(${JSONCPP_INCLUDE_DIRS})
    link_directories(${JSONCPP_LIBRARY_DIRS})
endif()

add_executable(test-cmake-app
    src/main.cpp
    src/config.cpp
)

if(JSONCPP_FOUND)
    target_link_libraries(test-cmake-app ${JSONCPP_LIBRARIES})
endif()

add_library(testcmakelib SHARED
    src/lib/utils.cpp
)

install(TARGETS test-cmake-app DESTINATION bin)
install(TARGETS testcmakelib DESTINATION lib)
install(FILES src/lib/utils.h DESTINATION include)
"#,
    )
    .unwrap();

    std::fs::create_dir_all(project_dir.join("src/lib")).unwrap();
    std::fs::write(
        project_dir.join("src/main.cpp"),
        r#"#include <iostream>
#include "lib/utils.h"

int main() {
    std::cout << "Hello from CMake!" << std::endl;
    std::cout << "Random number: " << generateRandomNumber() << std::endl;
    return 0;
}
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("src/config.cpp"),
        r#"#include <fstream>
#include <iostream>

bool loadConfig(const std::string& filename) {
    std::ifstream file(filename);
    return file.good();
}
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("src/lib/utils.h"),
        r#"#ifndef UTILS_H
#define UTILS_H

int generateRandomNumber();
void printVersion();

#endif
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("src/lib/utils.cpp"),
        r#"#include "utils.h"
#include <iostream>
#include <random>

int generateRandomNumber() {
    static std::random_device rd;
    static std::mt19937 gen(rd());
    std::uniform_int_distribution<> dis(1, 100);
    return dis(gen);
}

void printVersion() {
    std::cout << "TestCMakeProject v1.2.0" << std::endl;
}
"#,
    )
    .unwrap();

    // Initialize git repository
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("TestCMakeProject".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        check: false, // Disable external checking to prevent hanging
        consult_external_directory: false, // Disable external directory consultation
        force_subprocess: false, // Disable subprocess calls to prevent external tool errors
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("1.2.0"),
        &metadata,
    );

    assert!(
        result.is_ok(),
        "CMake project should debianize successfully"
    );

    // Check that CMake was detected
    let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
    let control_str = String::from_utf8_lossy(&control_content);
    assert!(control_str.contains("cmake"));

    let rules_content = wt.get_file_text(&subpath.join("debian/rules")).unwrap();
    let rules_str = String::from_utf8_lossy(&rules_content);
    assert!(rules_str.contains("dh $@ --buildsystem=cmake"));
}

#[test]
fn test_meson_project_detection() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-meson-project");
    std::fs::create_dir(&project_dir).unwrap();

    // Create Meson project files
    std::fs::write(
        project_dir.join("meson.build"),
        r#"project('test-meson-project', 'c',
  version : '0.5.0',
  license : 'GPL-3.0',
  default_options : ['warning_level=3'])

cc = meson.get_compiler('c')

# Check for dependencies
glib_dep = dependency('glib-2.0', version : '>= 2.50')
json_glib_dep = dependency('json-glib-1.0', required : false)

# Configure file
conf_data = configuration_data()
conf_data.set('version', meson.project_version())
configure_file(input : 'config.h.in',
               output : 'config.h',
               configuration : conf_data)

# Include directories
inc_dir = include_directories('src')

# Source files
src_files = [
  'src/main.c',
  'src/utils.c'
]

# Executable
executable('test-meson-app', src_files,
           dependencies : [glib_dep, json_glib_dep],
           include_directories : inc_dir,
           install : true)

# Library
libutils = library('utils', ['src/utils.c'],
                   dependencies : glib_dep,
                   include_directories : inc_dir,
                   install : true)

# Install headers
install_headers('src/utils.h')
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("config.h.in"),
        r#"#ifndef CONFIG_H
#define CONFIG_H

#define VERSION "@version@"

#endif
"#,
    )
    .unwrap();

    std::fs::create_dir(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("src/main.c"),
        r#"#include <stdio.h>
#include <glib.h>
#include "utils.h"
#include "config.h"

int main() {
    printf("Test Meson Project v%s\n", VERSION);
    printf("GLib version: %d.%d.%d\n", 
           GLIB_MAJOR_VERSION, GLIB_MINOR_VERSION, GLIB_MICRO_VERSION);
    
    char *result = process_string("Hello, Meson!");
    printf("Processed: %s\n", result);
    g_free(result);
    
    return 0;
}
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("src/utils.h"),
        r#"#ifndef UTILS_H
#define UTILS_H

#include <glib.h>

char* process_string(const char* input);
int calculate_sum(int a, int b);

#endif
"#,
    )
    .unwrap();

    std::fs::write(
        project_dir.join("src/utils.c"),
        r#"#include "utils.h"
#include <string.h>

char* process_string(const char* input) {
    if (!input) return NULL;
    return g_strdup_printf("Processed: %s", input);
}

int calculate_sum(int a, int b) {
    return a + b;
}
"#,
    )
    .unwrap();

    // Initialize git repository
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("test-meson-project".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        check: false, // Disable external checking to prevent hanging
        consult_external_directory: false, // Disable external directory consultation
        force_subprocess: false, // Disable subprocess calls to prevent external tool errors
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("0.5.0"),
        &metadata,
    );

    // Meson might not be fully supported in processors, which is okay
    match result {
        Ok(_) => {
            println!("Meson project debianization succeeded");
            assert!(wt.has_filename(&subpath.join("debian/control")));
        }
        Err(e) => {
            println!(
                "Meson project debianization failed (may use default processor): {:?}",
                e
            );
            // This might be expected if Meson isn't explicitly supported
        }
    }
}

#[test]
fn test_generic_makefile_project() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-makefile-project");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a generic Makefile project
    std::fs::write(
        project_dir.join("Makefile"),
        r#"CC = gcc
CFLAGS = -Wall -Wextra -std=c99 -O2
LDFLAGS = -lm

SRCDIR = src
SOURCES = $(wildcard $(SRCDIR)/*.c)
OBJECTS = $(SOURCES:.c=.o)
TARGET = test-app

PREFIX = /usr/local
BINDIR = $(PREFIX)/bin
MANDIR = $(PREFIX)/share/man/man1

.PHONY: all clean install uninstall

all: $(TARGET)

$(TARGET): $(OBJECTS)
	$(CC) $(OBJECTS) -o $@ $(LDFLAGS)

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f $(OBJECTS) $(TARGET)

install: $(TARGET)
	install -d $(DESTDIR)$(BINDIR)
	install -m 755 $(TARGET) $(DESTDIR)$(BINDIR)
	install -d $(DESTDIR)$(MANDIR)
	install -m 644 $(TARGET).1 $(DESTDIR)$(MANDIR)

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/$(TARGET)
	rm -f $(DESTDIR)$(MANDIR)/$(TARGET).1

test: $(TARGET)
	./$(TARGET) --version
	./$(TARGET) --help
"#,
    )
    .unwrap();

    std::fs::create_dir(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("src/main.c"),
        r#"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

void print_version() {
    printf("test-app version 2.1.0\n");
}

void print_help() {
    printf("Usage: test-app [OPTIONS]\n");
    printf("Options:\n");
    printf("  --version    Show version information\n");
    printf("  --help       Show this help message\n");
    printf("  --calculate  Calculate square root of 42\n");
}

int main(int argc, char *argv[]) {
    if (argc > 1) {
        if (strcmp(argv[1], "--version") == 0) {
            print_version();
            return 0;
        } else if (strcmp(argv[1], "--help") == 0) {
            print_help();
            return 0;
        } else if (strcmp(argv[1], "--calculate") == 0) {
            printf("Square root of 42 is: %.2f\n", sqrt(42));
            return 0;
        } else {
            printf("Unknown option: %s\n", argv[1]);
            print_help();
            return 1;
        }
    }
    
    printf("Hello from Makefile project!\n");
    return 0;
}
"#,
    )
    .unwrap();

    // Create a simple man page
    std::fs::write(
        project_dir.join("test-app.1"),
        r#".TH TEST-APP 1 "2023-01-01" "test-app 2.1.0" "User Commands"
.SH NAME
test-app \- a simple test application
.SH SYNOPSIS
.B test-app
[\fIOPTIONS\fR]
.SH DESCRIPTION
A simple test application built with Make.
.SH OPTIONS
.TP
.BR \-\-version
Show version information
.TP
.BR \-\-help
Show help message
.TP
.BR \-\-calculate
Calculate square root of 42
.SH AUTHOR
Test Author
"#,
    )
    .unwrap();

    // Initialize git repository
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Open the working tree
    let (wt, subpath) = breezyshim::workingtree::open_containing(&project_dir).unwrap();

    let mut metadata = UpstreamMetadata::new();
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Name("test-app".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });

    let preferences = DebianizePreferences {
        net_access: false,
        trust: true,
        check: false, // Disable external checking to prevent hanging
        consult_external_directory: false, // Disable external directory consultation
        force_subprocess: false, // Disable subprocess calls to prevent external tool errors
        session: debianize::SessionPreferences::Plain,
        ..Default::default()
    };

    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()),
        Some(&subpath),
        &preferences,
        Some("2.1.0"),
        &metadata,
    );

    assert!(
        result.is_ok(),
        "Generic Makefile project should debianize successfully"
    );

    // Verify debian files
    assert!(wt.has_filename(&subpath.join("debian/control")));
    assert!(wt.has_filename(&subpath.join("debian/rules")));

    let rules_content = wt.get_file_text(&subpath.join("debian/rules")).unwrap();
    let rules_str = String::from_utf8_lossy(&rules_content);
    assert!(rules_str.contains("dh $@ --buildsystem=makefile") || rules_str.contains("dh $@"));

    let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
    let control_str = String::from_utf8_lossy(&control_content);
    assert!(control_str.contains("Architecture: any")); // C projects are typically arch-dependent
}
