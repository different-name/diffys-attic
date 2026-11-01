func SkipIntrosOnSplash(controller: ref<SplashScreenLoadingScreenLogicController>) {
  controller.logosTrainAnimation = n"after_skip_pressed";
  controller.longLogosTrainAnimation = n"after_skip_pressed";
  controller.localizedMessageAnimation = n"after_skip_pressed";
  controller.gameIntroAnimation = n"after_skip_pressed";
}

@if(!ModuleExists("Codeware"))
public native class SplashScreenLoadingScreenLogicController extends ILoadingLogicController {
  public native let logosTrainAnimation: CName;
  public native let longLogosTrainAnimation: CName;
  public native let localizedMessageAnimation: CName;
  public native let gameIntroAnimation: CName;

  protected cb func OnInitialize() -> Bool {
    SkipIntrosOnSplash(this);
  }
}

// codeware declares the class, so hook the base and match the native name
@if(ModuleExists("Codeware"))
@addMethod(ILoadingLogicController)
protected cb func OnInitialize() -> Bool {
  if this.IsA(n"inkSplashScreenLoadingScreenLogicController") {
    SkipIntrosOnSplash(this as SplashScreenLoadingScreenLogicController);
  }
}
