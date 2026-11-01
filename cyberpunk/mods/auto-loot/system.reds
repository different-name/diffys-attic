public class AutoLootAddRequest extends ScriptableSystemRequest {
  public let source: wref<GameObject>;
}

public class AutoLootRemoveRequest extends ScriptableSystemRequest {
  public let source: wref<GameObject>;
}

public class AutoLootTickRequest extends ScriptableSystemRequest {
  public let generation: Int32;
}

public class AutoLootTickCallback extends DelayCallback {
  public let system: wref<AutoLootSystem>;
  public let generation: Int32;

  public func Call() -> Void {
    if IsDefined(this.system) {
      let request = new AutoLootTickRequest();
      request.generation = this.generation;
      this.system.QueueRequest(request);
    }
  }
}

public class AutoLootPass {
  public let player: wref<PlayerPuppet>;
  public let budget: Int32;
  public let objectivesGathered: Bool;
  public let objectivesAvailable: Bool;
  public let objectivePositions: array<Vector4>;
}

public class AutoLootSystem extends ScriptableSystem {
  private let m_sources: array<wref<GameObject>>;
  private let m_cursor: Int32;
  private let m_player: wref<PlayerPuppet>;
  private let m_running: Bool;
  private let m_generation: Int32;
  private let m_delayID: DelayID;

  public static func Get(game: GameInstance) -> ref<AutoLootSystem> {
    let container = GameInstance.GetScriptableSystemsContainer(game);
    if !IsDefined(container) {
      return null;
    }
    return container.Get(n"AutoLootSystem") as AutoLootSystem;
  }

  public static func NotifyAttached(object: ref<GameObject>) -> Void {
    if !AutoLootRules.IsCandidate(object) {
      return;
    }
    let system = AutoLootSystem.Get(object.GetGame());
    if IsDefined(system) {
      let request = new AutoLootAddRequest();
      request.source = object;
      system.QueueRequest(request);
    }
  }

  public static func NotifyDetached(object: ref<GameObject>) -> Void {
    if !AutoLootRules.IsCandidate(object) {
      return;
    }
    let system = AutoLootSystem.Get(object.GetGame());
    if IsDefined(system) {
      let request = new AutoLootRemoveRequest();
      request.source = object;
      system.QueueRequest(request);
    }
  }

  private func OnAttach() -> Void {
    this.StartScanner();
  }

  private func OnDetach() -> Void {
    this.StopScanner();
    ArrayClear(this.m_sources);
    this.m_cursor = 0;
  }

  private func OnRestored(saveVersion: Int32, gameVersion: Int32) -> Void {
    this.StartScanner();
  }

  private final func OnPlayerAttach(request: ref<PlayerAttachRequest>) -> Void {
    let player = request.owner as PlayerPuppet;
    if IsDefined(player) {
      this.m_player = player;
      this.StartScanner();
    }
  }

  private final func OnPlayerDetach(request: ref<PlayerDetachRequest>) -> Void {
    if IsDefined(this.m_player) && this.m_player.GetEntityID() != request.ownerID {
      return;
    }
    this.m_player = null;
    this.m_cursor = 0;
  }

  private final func OnAutoLootAddRequest(request: ref<AutoLootAddRequest>) -> Void {
    let source = request.source;
    if IsDefined(source) && !ArrayContains(this.m_sources, source) {
      ArrayPush(this.m_sources, source);
    }
  }

  private final func OnAutoLootRemoveRequest(request: ref<AutoLootRemoveRequest>) -> Void {
    let source = request.source;
    if !IsDefined(source) {
      return;
    }
    let index = ArrayFindFirst(this.m_sources, source);
    if index >= 0 {
      this.RemoveAt(index);
    }
  }

  private final func OnAutoLootTickRequest(request: ref<AutoLootTickRequest>) -> Void {
    if !this.m_running || request.generation != this.m_generation {
      return;
    }
    this.m_delayID = GetInvalidDelayID();
    this.RunPass();
    if this.m_running && request.generation == this.m_generation {
      this.Schedule();
    }
  }

  private final func StartScanner() -> Void {
    this.StopScanner();
    this.m_running = true;
    this.Schedule();
  }

  private final func StopScanner() -> Void {
    this.m_generation += 1;
    let delays = GameInstance.GetDelaySystem(this.GetGameInstance());
    if IsDefined(delays) && this.m_delayID != GetInvalidDelayID() {
      delays.CancelCallback(this.m_delayID);
    }
    this.m_delayID = GetInvalidDelayID();
    this.m_running = false;
  }

  private final func Schedule() -> Void {
    let delays = GameInstance.GetDelaySystem(this.GetGameInstance());
    if !IsDefined(delays) {
      return;
    }
    let callback = new AutoLootTickCallback();
    callback.system = this;
    callback.generation = this.m_generation;
    this.m_delayID = delays.DelayCallback(callback, AutoLootConfig.ScanInterval(), false);
  }

  private final func RemoveAt(index: Int32) -> Void {
    ArrayErase(this.m_sources, index);
    if index < this.m_cursor {
      this.m_cursor -= 1;
    }
  }

  private final func RunPass() -> Void {
    let game = this.GetGameInstance();
    let player = GetPlayer(game);
    if !AutoLootRules.GatesOpen(player, game) {
      return;
    }
    let pass = new AutoLootPass();
    pass.player = player;
    pass.budget = AutoLootConfig.MaxTransfersPerPass();
    let limit = Min(AutoLootConfig.MaxSourcesPerPass(), ArraySize(this.m_sources));
    let examined = 0;
    while examined < limit && pass.budget > 0 && ArraySize(this.m_sources) > 0 {
      if this.m_cursor >= ArraySize(this.m_sources) {
        this.m_cursor = 0;
      }
      let source = this.m_sources[this.m_cursor];
      examined += 1;
      if !IsDefined(source) {
        ArrayErase(this.m_sources, this.m_cursor);
      } else {
        this.m_cursor += 1;
        if source.IsAttached() {
          this.LootSource(source, pass, game);
        }
      }
    }
  }

  private final func LootSource(source: ref<GameObject>, pass: ref<AutoLootPass>, game: GameInstance) -> Void {
    let player: ref<PlayerPuppet> = pass.player;
    let sourcePosition = source.GetWorldPosition();
    if Vector4.Distance(player.GetWorldPosition(), sourcePosition) > AutoLootConfig.EffectiveRadius() {
      return;
    }
    if AutoLootRules.SourceProtected(source) || !AutoLootRules.SourceOpen(source, player) {
      return;
    }
    if !GameInstance.GetTargetingSystem(game).IsVisibleTarget(player, source) {
      return;
    }
    let transactions = GameInstance.GetTransactionSystem(game);
    let items: array<wref<gameItemData>>;
    transactions.GetItemList(source, items);
    if ArraySize(items) == 0 || AutoLootRules.HasQuestItem(items) {
      return;
    }
    let eligible: array<wref<gameItemData>>;
    for item in items {
      if AutoLootRules.ItemEligible(item, game) {
        ArrayPush(eligible, item);
      }
    }
    if ArraySize(eligible) == 0 || this.NearObjective(sourcePosition, pass, game) {
      return;
    }
    let moved = false;
    let soundTried = false;
    for item in eligible {
      if pass.budget <= 0 || !this.StillLootable(source, player) {
        break;
      }
      if AutoLootRules.ItemEligible(item, game) {
        // hold a strong ref, money shards and broken weapons can be consumed once they land
        let captured: ref<gameItemData> = item;
        pass.budget -= 1;
        if transactions.TransferItem(source, player, captured.GetID(), captured.GetQuantity()) {
          moved = true;
          if !soundTried {
            soundTried = true;
            this.PlayLootSound(captured, game);
          }
        }
      }
    }
    if moved {
      this.OpenAnimatedContainer(source);
    }
  }

  private final func StillLootable(source: ref<GameObject>, player: ref<PlayerPuppet>) -> Bool {
    return source.IsAttached()
      && IsDefined(player)
      && player.IsAttached()
      && !source.IsQuest()
      && !source.IsAnyClueEnabled();
  }

  private final func NearObjective(position: Vector4, pass: ref<AutoLootPass>, game: GameInstance) -> Bool {
    if !pass.objectivesGathered {
      pass.objectivesGathered = true;
      pass.objectivesAvailable = AutoLootRules.GatherObjectivePositions(game, pass.objectivePositions);
    }
    if !pass.objectivesAvailable {
      return true;
    }
    let exclusion = AutoLootConfig.ObjectiveExclusionDistance();
    for pin in pass.objectivePositions {
      if Vector4.Distance(position, pin) <= exclusion {
        return true;
      }
    }
    return false;
  }

  private final func PlayLootSound(item: ref<gameItemData>, game: GameInstance) -> Void {
    if !AutoLootConfig.PickupSound() || !IsDefined(item) {
      return;
    }
    let audio = GameInstance.GetAudioSystem(game);
    if IsDefined(audio) {
      audio.PlayItemActionSound(n"Loot", item);
    }
  }

  private final func OpenAnimatedContainer(source: ref<GameObject>) -> Void {
    let animated = source as LootContainerObjectAnimatedByTransform;
    if IsDefined(animated) && animated.IsAttached() {
      animated.AutoLootOpenOnce();
    }
  }
}
