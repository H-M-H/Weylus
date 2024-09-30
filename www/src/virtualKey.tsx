import { h, render } from "preact";
import { memo, useCallback, useEffect, useMemo, useRef } from "preact/compat";
import { batch, computed, effect, signal, useComputed, useSignal } from "@preact/signals";
import { approx, clsx, is, startMouseMove } from "yon-utils";
import { KEvent, sendWsMessage } from ".";

interface VirtualKeyItem {
  x: number; // percentage 0~100
  y: number; // percentage 0~100
  width: number; // in pixels
  height: number; // in pixels

  kEvent: KEvent | null; // ignoring its "event_type"
}

interface VirtualKeyProfile {
  name: string;
  items: VirtualKeyItem[];
}

const profiles = signal<VirtualKeyProfile[]>([]);
const profileIndex = signal(Number(localStorage.getItem("profileIndex") ?? "-1"));

effect(() => localStorage.setItem("profileIndex", profileIndex.value.toString()))
const currentProfile = computed(() => profiles.value[profileIndex.value]);

const isEditing = signal(false);
const currentIndex = signal(-1);
effect(() => {
  const itemsCount = currentProfile.value?.items.length ?? 0;
  if (currentIndex.value >= itemsCount) currentIndex.value = itemsCount - 1;
})

let editingPointerDown = false;

// the component to render a single virtual key
const VirtualKeyItem = memo((_props: {
  item: VirtualKeyItem;
  index: number;
}) => {
  const props = useRef(_props); props.current = _props;
  const item = _props.item;

  const justPressed = useSignal(false)
  const inCombo = useSignal(false)

  const style = useMemo(() => ({
    left: `${item.x}%`,
    top: `${item.y}%`,
    width: `${item.width}px`,
    height: `${item.height}px`,
  }), [item])
  const text = useMemo(() => {
    let text = '';
    const kEvent = item.kEvent;
    if (kEvent) {
      if (kEvent.alt) text += 'Alt+';
      if (kEvent.ctrl) text += 'Ctrl+';
      if (kEvent.shift) text += 'Shift+';
      if (kEvent.meta) text += 'Meta+';
      text += kEvent.code;
    } else {
      text = '<not set>';
    }
    return text;
  }, [item.kEvent])

  const pointerDown = useCallback((e: PointerEvent) => {
    if (editingPointerDown) return

    const item = props.current.item
    const pointerId = e.pointerId;
    (e.currentTarget as HTMLElement).setPointerCapture(pointerId);
    e.stopPropagation();
    e.preventDefault();

    currentIndex.value = props.current.index;

    // for editing mode
    if (isEditing.value) {
      editingPointerDown = true;

      const { x, y, width, height } = item;

      e.preventDefault();
      e.stopPropagation();
      startMouseMove({
        initialEvent: e,
        onMove({ deltaX, deltaY }) {
          updateKeyItemNoCommit({
            x: x + deltaX / window.innerWidth * 100,
            y: y + deltaY / window.innerHeight * 100,
          })
        },
        onEnd({ deltaX, deltaY }) {
          editingPointerDown = false;
          if (!(approx(deltaX, 0) && approx(deltaY, 0))) commitVirtualKeysProfiles()
        }
      })

      return
    }

    // for regular clicks
    if (!item.kEvent) return

    // regular send key 
    const makePress = () => {
      justPressed.value = true;
      sendWsMessage({ "KeyboardEvent": item.kEvent });
      setTimeout(() => {
        justPressed.value = false;
        sendWsMessage({ "KeyboardEvent": { ...item.kEvent, event_type: "up" } });
      }, 70);
    }

    makePress()

    let killPressingStrikeTimer: () => void;
    const timer0 = setTimeout(() => {
      inCombo.value = true;
      const timer1 = setInterval(makePress, 120);
      killPressingStrikeTimer = () => {
        clearInterval(timer1)
        inCombo.value = false;
      };
    }, 800); // if press for too long, start repeating the keypress
    killPressingStrikeTimer = () => clearTimeout(timer0);

    function handleGlobalPointerUp(e: PointerEvent) {
      if (e.pointerId !== pointerId) return
      killPressingStrikeTimer();
      document.removeEventListener("pointerup", handleGlobalPointerUp, true);
      document.removeEventListener("pointercancel", handleGlobalPointerUp, true);
    }
    document.addEventListener("pointerup", handleGlobalPointerUp, true);
    document.addEventListener("pointercancel", handleGlobalPointerUp, true);
  }, []);

  return <div class={clsx(
    "vk-key",
    currentIndex.value === props.current.index && "isActive",
    justPressed.value && "justPressed",
    inCombo.value && "inCombo",
  )} style={style} onPointerDown={pointerDown}>
    {text}
  </div>
})

// the component to render the whole profile
const VirtualKeyProfileRender = (() => {
  const shallHijackGlobalKeyDown = useComputed(() => !!(isEditing.value && currentProfile.value?.items[currentIndex.value])).value
  useEffect(() => {
    if (!shallHijackGlobalKeyDown) return

    const globalKeydownHandler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      updateKeyItemNoCommit({ kEvent: new KEvent("down", e) })
      commitVirtualKeysProfiles();
    }

    document.addEventListener("keydown", globalKeydownHandler, true);
    return () => document.removeEventListener("keydown", globalKeydownHandler, true);
  }, [shallHijackGlobalKeyDown])

  const profile = currentProfile.value;
  return <div>
    {profile?.items.map((item, i) => <VirtualKeyItem key={i} item={item} index={i} />)}
  </div>
})

const VirtualKeyApp = () => {
  return <div id="vk-container" class={clsx(isEditing.value && "isEditing")}>
    {
      isEditing.value && (
        currentProfile.value ? (
          <div class="vk-editing-notice">
            <strong>Editing Virtual Keys Profile [{currentProfile.value.name}]</strong>:
            <button onClick={renameProfile}>Rename</button>
            <button onClick={deleteProfile}>Delete Profile</button>


            <br />
            <button onClick={addVirtualKeyItem}>Add Key</button>
            <button onClick={deleteVirtualKeyItem}>Delete</button>, drag to move, press keyboard to change key.
          </div>
        ) : (
          <div class="vk-editing-notice">
            <strong>No Virtual Key Profile</strong>: Add a profile first.
            <br />
            <button onClick={createProfile}>Create Profile</button>
          </div>
        )
      )
    }

    <VirtualKeyProfileRender />
  </div>
}

const ProfileSwitch = memo(() => {
  return <div style={{ display: "flex" }}>
    <select
      style={{ minWidth: "0", flex: "1 0 0" }}
      value={profileIndex.value}
      onChange={(e) => profileIndex.value = Number(e.currentTarget.value)}
    >
      <option value={-1}>None</option>
      {profiles.value.map((profile, i) => <option value={i}>{i}. {profile.name}</option>)}
    </select>

    <button
      style={{ marginTop: '0', marginLeft: '2px' }}
      title="Create Profile"
      onClick={createProfile}
    > + </button>
  </div>
})

const VirtualKeySettingsApp = () => {
  const editingCheck = useComputed(() => profiles.value.length > 0 && <label>
    <input type="checkbox"
      checked={isEditing.value}
      onChange={(e) => isEditing.value = e.currentTarget.checked}
    />
    <span>Edit Mode</span>
  </label>)

  return <div>
    <ProfileSwitch />
    {editingCheck.value}
  </div>
}

function createProfile() {
  const newProfiles = [
    ...profiles.value,
    { name: "New Profile", items: [] }
  ];

  batch(() => {
    profiles.value = newProfiles;
    isEditing.value = true;
    currentIndex.value = -1;
    profileIndex.value = newProfiles.length - 1;
  });
  commitVirtualKeysProfiles();
}

export function initVirtualKey() {
  render(<VirtualKeySettingsApp />, document.getElementById("vk-settings-container") as HTMLDivElement);
  render(<VirtualKeyApp />, document.getElementById("vk-container-outter") as HTMLDivElement);
}

export function setVirtualKeysProfiles(newProfiles: VirtualKeyProfile[]) {
  profiles.value = newProfiles;
}

function commitVirtualKeysProfiles() {
  console.log("committing profiles", profiles.value);
  sendWsMessage({ "SetVirtualKeysProfiles": JSON.stringify(profiles.value) });
}

function addVirtualKeyItem() {
  batch(() => {
    const profile = currentProfile.value;
    if (!profile) return;

    currentIndex.value = profile.items.length
    updateProfileNoCommit({
      items: [
        ...profile.items,
        {
          x: 50,
          y: 50,
          width: 200,
          height: 100,
          kEvent: null,
        }
      ]
    })
  })

  commitVirtualKeysProfiles();
}

function deleteVirtualKeyItem() {
  batch(() => {
    const profile = currentProfile.value;
    if (!profile) return;
    if (!profile.items.length) return;

    const $items = profile.items.slice();
    $items.splice(currentIndex.value, 1);

    if (currentIndex.value >= $items.length) currentIndex.value = $items.length - 1;
    updateProfileNoCommit({ items: $items })
  })

  commitVirtualKeysProfiles();
}

function renameProfile() {
  if (!currentProfile.value) return;
  const newName = prompt("New name for profile:", currentProfile.value.name);
  if (!newName) return;

  batch(() => {
    const profile = currentProfile.value;
    if (!profile) return;

    const $profiles = profiles.value.slice();
    $profiles[profileIndex.value] = { ...profile, name: newName };
    profiles.value = $profiles;
  })

  commitVirtualKeysProfiles();
}

function deleteProfile() {
  if (!confirm("Are you sure you want to delete this profile?")) return;

  batch(() => {
    if (!currentProfile.value) return;

    const $profiles = profiles.value.slice();
    $profiles.splice(profileIndex.value, 1);

    profileIndex.value = Math.max(profileIndex.value, $profiles.length - 1);
    currentIndex.value = -1;
    profiles.value = $profiles;
  })
  commitVirtualKeysProfiles();
}


// -----------------------------------------------------------------------------

function updateProfileNoCommit(partial: Partial<VirtualKeyProfile>) {
  // NOTE: without commit!
  const profile = currentProfile.value;
  if (!profile) return;

  const $profiles = profiles.value.slice();
  $profiles[profileIndex.value] = { ...profile, ...partial };
  profiles.value = $profiles;
}

function updateKeyItemNoCommit(partial: Partial<VirtualKeyItem>) {
  // NOTE: without commit!
  const profile = currentProfile.value;
  if (!profile) return;

  const items = profile.items.slice();
  const item = items[currentIndex.value];
  if (!item) return;

  items[currentIndex.value] = { ...item, ...partial };
  updateProfileNoCommit({ items });
}
