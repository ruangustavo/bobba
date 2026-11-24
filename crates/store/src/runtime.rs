use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runtime {
    Java,
    NodeJs,
    Python,
    DotNet,
}

impl Runtime {
    pub fn name(self) -> &'static str {
        match self {
            Runtime::Java => "Java",
            Runtime::NodeJs => "Node.js",
            Runtime::Python => "Python",
            Runtime::DotNet => ".NET",
        }
    }

    pub fn requirement(self) -> &'static str {
        match self {
            Runtime::Java => "Java with JavaFX",
            other => other.name(),
        }
    }

    pub fn program(self) -> &'static str {
        match self {
            Runtime::Java => "java",
            Runtime::NodeJs => "node",
            Runtime::Python => "python",
            Runtime::DotNet => "dotnet",
        }
    }

    pub fn guide(self) -> &'static str {
        match self {
            Runtime::Java => "https://www.azul.com/downloads/?package=jdk-fx#zulu",
            Runtime::NodeJs => "https://nodejs.org/en/download",
            Runtime::Python => "https://www.python.org/downloads/",
            Runtime::DotNet => "https://dotnet.microsoft.com/download",
        }
    }

    pub fn of_program(program: &str) -> Option<Self> {
        let stem = Path::new(program)
            .file_stem()?
            .to_str()?
            .to_ascii_lowercase();
        match stem.as_str() {
            "java" | "javaw" => Some(Runtime::Java),
            "node" => Some(Runtime::NodeJs),
            "python" | "python3" | "py" => Some(Runtime::Python),
            "dotnet" => Some(Runtime::DotNet),
            _ => None,
        }
    }

    pub fn of_framework(framework: &str) -> Option<Self> {
        match framework {
            "G-Node" => Some(Runtime::NodeJs),
            "G-Python" => Some(Runtime::Python),
            "QX" | "Geode" => Some(Runtime::DotNet),
            _ => None,
        }
    }
}
