//! The reference-stack behavior ports (goober's python/dotnet/java and
//! iOS-simulator integration files): the pipeline's check step runs the
//! stack's real verify command inside the run's worktree and comes back
//! green, and the decision gates a failing check to abort with its
//! diagnostics. The toolchain legs are opt-in (`BUREAU_E2E_*`), exactly
//! like goober's `RequireEnv` gate; the gate legs run offline.

#[path = "behavior/polyglot_support.rs"]
mod support;

use bureau::contract::StepOutcome;
use bureau::runlog::{self, RunState, RunStatus};
use support::Rig;

/// The change every green run leaves, so `done` lands a PR.
const CHANGE: &str = "echo change >> CHANGELOG.md";

/// The Python leg's seeded project.
const PYTHON_PROJECT: &[(&str, &str)] = &[(
    "test_answer.py",
    "def test_answer():\n    assert 42 == 42\n",
)];

const SERVICE_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>"#;

const TESTS_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup><ItemGroup><PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.*" /><PackageReference Include="xunit" Version="2.*" /></ItemGroup><ItemGroup><ProjectReference Include="../src/Service.csproj" /></ItemGroup></Project>"#;

const GREETER_CS: &str =
    "namespace StackFixture; public static class Greeter { public static int Answer() => 42; }";

const GREETER_TESTS_CS: &str = "using Xunit; using StackFixture; public class GreeterTests { [Fact] public void Answers() => Assert.Equal(42, Greeter.Answer()); }";

const POM_XML: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion><groupId>dev.bureau</groupId><artifactId>fixture</artifactId><version>1.0</version><properties><maven.compiler.release>21</maven.compiler.release></properties><dependencies><dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>5.10.2</version><scope>test</scope></dependency></dependencies></project>"#;

const GREETER_JAVA: &str = "public class Greeter { public static int answer() { return 42; } }";

const GREETER_TEST_JAVA: &str = "import org.junit.jupiter.api.Test; import static org.junit.jupiter.api.Assertions.assertEquals; class GreeterTest { @Test void answers() { assertEquals(42, Greeter.answer()); } }";

/// One opted-in toolchain leg: seed the project, run change -> check ->
/// verdict, and prove the real verify command came back green and the
/// run landed its PR.
async fn run_green(files: &[(&str, &str)], check: &str) {
    let rig = Rig::new(files);
    let plan = rig.plan(support::check_steps(CHANGE, check));
    let outcome = rig.engine().run(&plan).await;
    let seen = (
        outcome.outcome,
        support::check_outcome(&rig.events(&outcome.run_id)),
        outcome.pr.is_some(),
    );
    assert_eq!(
        seen,
        (StepOutcome::Success, Some("success".to_owned()), true)
    );
}

/// goober's python-gaggle integration port, gated the same way.
#[tokio::test]
async fn python_reference_verify_runs_green() {
    if !support::opted_in("BUREAU_E2E_PYTHON") {
        return;
    }
    support::require(&["python3", "-m", "pytest", "--version"]);
    run_green(PYTHON_PROJECT, "python3 -m pytest -q").await;
}

/// goober's dotnet-gaggle integration port, gated the same way. The
/// real `dotnet test` restores `NuGet` packages over the network, so the
/// leg stays out of the offline gate by design.
#[tokio::test]
async fn dotnet_reference_verify_runs_green() {
    if !support::opted_in("BUREAU_E2E_DOTNET") {
        return;
    }
    support::require(&["dotnet", "--version"]);
    let files = [
        ("src/Service.csproj", SERVICE_CSPROJ),
        ("src/Greeter.cs", GREETER_CS),
        ("tests/Tests.csproj", TESTS_CSPROJ),
        ("tests/GreeterTests.cs", GREETER_TESTS_CS),
    ];
    run_green(&files, "dotnet test tests/Tests.csproj").await;
}

/// goober's java-gaggle integration port, gated the same way.
#[tokio::test]
async fn java_reference_verify_runs_green() {
    if !support::opted_in("BUREAU_E2E_JAVA") {
        return;
    }
    support::require(&["mvn", "-v"]);
    let files = [
        ("pom.xml", POM_XML),
        ("src/main/java/Greeter.java", GREETER_JAVA),
        ("src/test/java/GreeterTest.java", GREETER_TEST_JAVA),
    ];
    run_green(&files, "mvn -B -q verify").await;
}

/// The recorded stdout output of the finished `check` step.
fn check_stdout(state: &RunState) -> Option<String> {
    state
        .steps
        .iter()
        .find(|record| record.step == "check")
        .and_then(|record| record.result.clone())
        .and_then(|result| result.outputs.get("stdout").cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
}

/// The result-gate green port (goober's iOS green run): the check's
/// success routes through the verdict to `done`, and its recorded
/// outputs carry the tool's own result line.
#[tokio::test]
async fn a_passing_check_routes_done_and_records_its_outputs() {
    let rig = Rig::new(&[("README.md", "fixture\n")]);
    let steps = support::check_steps(CHANGE, "echo '1 passed, 0 failed'");
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    let dir = rig.dir.path().join("runs").join(&outcome.run_id);
    let state = runlog::replay_state(&dir).expect("state replays");
    let seen = (outcome.outcome, outcome.pr.is_some(), check_stdout(&state));
    assert_eq!(
        seen,
        (
            StepOutcome::Success,
            true,
            Some("1 passed, 0 failed".to_owned())
        ),
        "the green check's outputs are recorded on the log"
    );
}

/// The result-gate failure port (goober's iOS failing-gate run): the
/// check's failure routes through the verdict to `abort`, and the
/// step's diagnostics are on the log.
#[tokio::test]
async fn a_failing_check_aborts_with_its_diagnostics() {
    let rig = Rig::new(&[("README.md", "fixture\n")]);
    let check = "echo 'FAILED test_answer: expected 42' >&2; exit 1";
    let plan = rig.plan(support::check_steps(CHANGE, check));
    let outcome = rig.engine().run(&plan).await;
    let dir = rig.dir.path().join("runs").join(&outcome.run_id);
    let state = runlog::replay_state(&dir).expect("state replays");
    let message = state
        .steps
        .iter()
        .find(|record| record.step == "check")
        .and_then(|record| record.result.clone())
        .map(|result| result.message);
    let seen = (outcome.outcome, outcome.pr.is_none(), state.status);
    assert_eq!(seen.0, StepOutcome::Failure);
    assert_eq!(
        (seen.1, seen.2, message.as_deref()),
        (
            true,
            RunStatus::Finished(StepOutcome::Failure),
            Some("FAILED test_answer: expected 42")
        ),
        "the failing check's diagnostics are recorded"
    );
}
