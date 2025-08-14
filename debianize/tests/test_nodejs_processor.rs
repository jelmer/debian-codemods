use breezyshim::tree::Tree;
use breezyshim::workingtree::WorkingTree;
use debianize::DebianizePreferences;
use tempfile::TempDir;
use upstream_ontologist::{Certainty, Origin, UpstreamDatum, UpstreamDatumWithMetadata, UpstreamMetadata};

#[test]
fn test_nodejs_project_debianization() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-nodejs-project");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a basic Node.js package.json
    std::fs::write(
        project_dir.join("package.json"),
        r#"{
  "name": "test-nodejs-package",
  "version": "1.2.3",
  "description": "A test Node.js package for debianization",
  "main": "index.js",
  "scripts": {
    "test": "jest",
    "start": "node index.js",
    "lint": "eslint ."
  },
  "keywords": ["test", "nodejs", "debian"],
  "author": "Test Author <test@example.com>",
  "license": "MIT",
  "dependencies": {
    "express": "^4.18.0",
    "lodash": "^4.17.21"
  },
  "devDependencies": {
    "jest": "^28.0.0",
    "eslint": "^8.0.0"
  },
  "engines": {
    "node": ">=14.0.0"
  }
}"#,
    )
    .unwrap();

    // Create main JavaScript file
    std::fs::write(
        project_dir.join("index.js"),
        r#"
const express = require('express');
const _ = require('lodash');

const app = express();
const PORT = process.env.PORT || 3000;

app.get('/', (req, res) => {
    const data = { message: 'Hello, World!', timestamp: new Date() };
    res.json(_.pick(data, ['message', 'timestamp']));
});

app.listen(PORT, () => {
    console.log(`Server running on port ${PORT}`);
});

module.exports = app;
"#,
    )
    .unwrap();

    // Create a test file
    std::fs::create_dir(project_dir.join("test")).unwrap();
    std::fs::write(
        project_dir.join("test/index.test.js"),
        r#"
const request = require('supertest');
const app = require('../index');

describe('GET /', () => {
    it('should return a JSON response', async () => {
        const response = await request(app)
            .get('/')
            .expect(200)
            .expect('Content-Type', /json/);
        
        expect(response.body).toHaveProperty('message', 'Hello, World!');
        expect(response.body).toHaveProperty('timestamp');
    });
});
"#,
    )
    .unwrap();

    // Create README
    std::fs::write(
        project_dir.join("README.md"),
        "# Test Node.js Package\n\nA test Node.js package for debianization testing.\n",
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
        datum: UpstreamDatum::Name("test-nodejs-package".to_string()),
        certainty: Some(Certainty::Confident),
        origin: Some(Origin::Other("test".to_string())),
    });
    metadata.insert(UpstreamDatumWithMetadata {
        datum: UpstreamDatum::Version("1.2.3".to_string()),
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

    // Run debianize
    let result = debianize::debianize(
        &wt,
        &subpath,
        Some(&wt.branch()), // use local branch as upstream
        Some(&subpath), // upstream subpath
        &preferences,
        None, // no upstream version override
        &metadata,
    );

    match result {
        Ok(debianize_result) => {
            println!("Node.js debianization successful: {:?}", debianize_result);

            // Verify debian directory was created
            assert!(wt.has_filename(&subpath.join("debian")));
            assert!(wt.has_filename(&subpath.join("debian/control")));
            assert!(wt.has_filename(&subpath.join("debian/rules")));
            assert!(wt.has_filename(&subpath.join("debian/changelog")));

            // Check control file contents
            let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
            let control_str = String::from_utf8_lossy(&control_content);
            
            // Should follow Node.js naming conventions
            assert!(control_str.contains("Source: node-test-nodejs-package"));
            assert!(control_str.contains("Package: node-test-nodejs-package"));
            
            // Should contain Node.js-specific dependencies
            assert!(control_str.contains("dh-sequence-nodejs"));
            
            // Should be architecture all for most Node.js packages
            assert!(control_str.contains("Architecture: all"));
            
            // Should have Node.js testsuite
            assert!(control_str.contains("Testsuite: autopkgtest-pkg-nodejs"));

            // Check rules file
            let rules_content = wt.get_file_text(&subpath.join("debian/rules")).unwrap();
            let rules_str = String::from_utf8_lossy(&rules_content);
            assert!(rules_str.contains("dh $@"));
        }
        Err(e) => {
            panic!("Node.js debianization failed: {:?}", e);
        }
    }
}

#[test]
fn test_scoped_nodejs_package() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-scoped-package");
    std::fs::create_dir(&project_dir).unwrap();

    // Create a scoped Node.js package
    std::fs::write(
        project_dir.join("package.json"),
        r#"{
  "name": "@myorg/test-package",
  "version": "0.5.1",
  "description": "A scoped test package",
  "main": "lib/index.js",
  "repository": {
    "type": "git",
    "url": "https://github.com/myorg/test-package.git"
  },
  "author": "Test Organization",
  "license": "Apache-2.0",
  "dependencies": {
    "uuid": "^9.0.0"
  }
}"#,
    )
    .unwrap();

    std::fs::create_dir(project_dir.join("lib")).unwrap();
    std::fs::write(
        project_dir.join("lib/index.js"),
        r#"
const { v4: uuidv4 } = require('uuid');

function generateId() {
    return uuidv4();
}

module.exports = { generateId };
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
        datum: UpstreamDatum::Name("@myorg/test-package".to_string()),
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
        Some("0.5.1"),
        &metadata,
    );

    match result {
        Ok(_) => {
            // Success - continue with the test
        }
        Err(e) => {
            panic!("Scoped Node.js package debianization failed: {:?}", e);
        }
    }

    // Check that scoped package name is handled correctly
    let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
    let control_str = String::from_utf8_lossy(&control_content);
    
    // Scoped packages should have @ stripped and / converted to -
    assert!(control_str.contains("Source: node-myorg-test-package"));
    assert!(control_str.contains("Package: node-myorg-test-package"));
}

#[test]
fn test_nodejs_package_with_typescript() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-typescript-package");
    std::fs::create_dir(&project_dir).unwrap();

    // Create TypeScript Node.js package
    std::fs::write(
        project_dir.join("package.json"),
        r#"{
  "name": "typescript-test-package",
  "version": "2.0.0",
  "description": "A TypeScript Node.js package",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc",
    "test": "jest",
    "prepare": "npm run build"
  },
  "author": "TS Author",
  "license": "MIT",
  "dependencies": {
    "axios": "^1.0.0"
  },
  "devDependencies": {
    "@types/node": "^18.0.0",
    "typescript": "^4.8.0",
    "jest": "^29.0.0"
  }
}"#,
    )
    .unwrap();

    // Create tsconfig.json
    std::fs::write(
        project_dir.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true,
    "declaration": true,
    "esModuleInterop": true
  },
  "include": ["src/**/*"],
  "exclude": ["node_modules", "dist"]
}"#,
    )
    .unwrap();

    // Create source files
    std::fs::create_dir(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("src/index.ts"),
        r#"
import axios from 'axios';

export interface ApiResponse<T> {
    data: T;
    status: number;
}

export async function fetchData<T>(url: string): Promise<ApiResponse<T>> {
    const response = await axios.get<T>(url);
    return {
        data: response.data,
        status: response.status
    };
}

export function greet(name: string): string {
    return `Hello, ${name}!`;
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
        datum: UpstreamDatum::Name("typescript-test-package".to_string()),
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
        Some("2.0.0"),
        &metadata,
    );

    match result {
        Ok(_) => {
            // Success - continue with the test
        }
        Err(e) => {
            panic!("TypeScript Node.js package debianization failed: {:?}", e);
        }
    }

    // Verify debian files
    assert!(wt.has_filename(&subpath.join("debian/control")));
    
    let control_content = wt.get_file_text(&subpath.join("debian/control")).unwrap();
    let control_str = String::from_utf8_lossy(&control_content);
    assert!(control_str.contains("node-typescript-test-package"));
}