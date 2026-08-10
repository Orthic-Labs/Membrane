#[test]
fn onboarding_choice_membrane_only_fails() {
    // AC-6: OnboardingChoice::MembraneOnly must fail to compile — verified via trybuild.
    // tests/ui/onboarding_membrane_only.rs references the nonexistent variant and must be rejected by rustc.
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/onboarding_membrane_only.rs");
}
