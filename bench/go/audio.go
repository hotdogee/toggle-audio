// Core Audio operations behind the four bench commands. Every HRESULT is
// checked; every interface, id string and PROPVARIANT is released on every
// path (success and failure) before the session's CoUninitialize.

package main

import (
	"runtime"
	"strconv"
	"syscall"
	"unsafe"
)

// exitError is a failure that ends the process with a specific exit code
// after printing msg on stderr.
type exitError struct {
	code int
	msg  string
}

// comFailure reports a failed COM call as "error: <step> hr=0x%08X" (exit 2).
func comFailure(step string, hr hresult) *exitError {
	return &exitError{code: exitCOM, msg: "error: " + step + " hr=0x" + hex8(uint32(hr))}
}

// audioSession owns the COM apartment and the IMMDeviceEnumerator.
type audioSession struct {
	enumerator   *comObject
	uninitialize bool // CoInitializeEx returned S_OK or S_FALSE and must be paired
}

// openAudioSession initializes COM (STA, as the product does) and creates the
// device enumerator, stamping the com_init and enumerator phases.
func openAudioSession(t *timer) (*audioSession, *exitError) {
	s := &audioSession{}
	hr := callProc(procCoInitializeEx, 0, coinitApartmentThreaded|coinitDisableOLE1DDE)
	switch {
	case hr == hrRPCChangedMode:
		// The thread is already in the MTA. Both CLSIDs are ThreadingModel=Both,
		// so COM is usable; we just must not call CoUninitialize.
	case hr.failed():
		return nil, comFailure("CoInitializeEx", hr)
	default: // S_OK or S_FALSE
		s.uninitialize = true
	}
	t.mark("com_init")

	en, hr := coCreateInstance(&clsidMMDeviceEnumerator, &iidIMMDeviceEnumerator)
	if hr.failed() {
		s.close()
		return nil, comFailure("CoCreateInstance(MMDeviceEnumerator)", hr)
	}
	s.enumerator = en
	t.mark("enumerator")
	return s, nil
}

// close releases the enumerator and leaves the COM apartment.
func (s *audioSession) close() {
	s.enumerator.release()
	s.enumerator = nil
	if s.uninitialize {
		callProc(procCoUninitialize)
		s.uninitialize = false
	}
}

// deviceID returns IMMDevice::GetId as a Go string (the CoTaskMem buffer is freed).
func deviceID(dev *comObject) (string, *exitError) {
	var p *uint16
	if hr := dev.call(slotGetID, uintptr(unsafe.Pointer(&p))); hr.failed() {
		return "", comFailure("IMMDevice::GetId", hr)
	}
	return takeCoTaskString(p), nil
}

// friendlyName reads PKEY_Device_FriendlyName, the string Windows Sound
// settings shows (on this zh-TW machine e.g. "喇叭 (FiiO BTA30 PRO)").
// A missing or non-string value yields "".
func friendlyName(dev *comObject) (string, *exitError) {
	var store *comObject
	if hr := dev.call(slotOpenPropertyStore, stgmRead, uintptr(unsafe.Pointer(&store))); hr.failed() {
		return "", comFailure("IMMDevice::OpenPropertyStore", hr)
	}
	defer store.release()

	var pv propVariant // the zero value is what PropVariantInit produces
	hr := store.call(slotPropertyStoreGetValue,
		uintptr(unsafe.Pointer(&pkeyDeviceFriendlyName)), uintptr(unsafe.Pointer(&pv)))
	if hr.failed() {
		return "", comFailure("IPropertyStore::GetValue(PKEY_Device_FriendlyName)", hr)
	}
	name := ""
	if pv.vt == vtLPWSTR {
		name = utf16PtrToString(pv.pwszVal)
	}
	if hr := callProc(procPropVariantClear, uintptr(unsafe.Pointer(&pv))); hr.failed() {
		return "", comFailure("PropVariantClear", hr)
	}
	return name, nil
}

// defaultDevice returns the default render endpoint for role, or (nil, nil)
// if there is none (E_NOTFOUND). The caller releases the returned device.
func (s *audioSession) defaultDevice(role uintptr) (*comObject, *exitError) {
	var dev *comObject
	hr := s.enumerator.call(slotGetDefaultAudioEndpoint, eRender, role, uintptr(unsafe.Pointer(&dev)))
	if hr == hrNotFound {
		return nil, nil
	}
	if hr.failed() {
		return nil, comFailure("IMMDeviceEnumerator::GetDefaultAudioEndpoint", hr)
	}
	return dev, nil
}

// defaultID returns the id of the default render endpoint for role, or ""
// if there is no default device.
func (s *audioSession) defaultID(role uintptr) (string, *exitError) {
	dev, err := s.defaultDevice(role)
	if err != nil || dev == nil {
		return "", err
	}
	defer dev.release()
	return deviceID(dev)
}

// list implements `list`: one "<id>\t<name>\t<flags>" line per ACTIVE render
// endpoint, in enumeration order.
func (s *audioSession) list() ([]byte, *exitError) {
	consoleID, err := s.defaultID(eConsole)
	if err != nil {
		return nil, err
	}
	commID, err := s.defaultID(eCommunications)
	if err != nil {
		return nil, err
	}

	var coll *comObject
	hr := s.enumerator.call(slotEnumAudioEndpoints, eRender, deviceStateActive, uintptr(unsafe.Pointer(&coll)))
	if hr.failed() {
		return nil, comFailure("IMMDeviceEnumerator::EnumAudioEndpoints", hr)
	}
	defer coll.release()

	var count uint32
	if hr := coll.call(slotCollectionGetCount, uintptr(unsafe.Pointer(&count))); hr.failed() {
		return nil, comFailure("IMMDeviceCollection::GetCount", hr)
	}

	out := make([]byte, 0, 128*int(count))
	for i := uint32(0); i < count; i++ {
		id, name, err := collectionEntry(coll, i)
		if err != nil {
			return nil, err
		}
		isDefault := consoleID != "" && id == consoleID
		isComm := commID != "" && id == commID
		out = appendListLine(out, id, name, roleFlags(isDefault, isComm))
	}
	return out, nil
}

// collectionEntry returns the id and friendly name of item i of coll.
func collectionEntry(coll *comObject, i uint32) (id, name string, err *exitError) {
	var dev *comObject
	if hr := coll.call(slotCollectionItem, uintptr(i), uintptr(unsafe.Pointer(&dev))); hr.failed() {
		return "", "", comFailure("IMMDeviceCollection::Item", hr)
	}
	defer dev.release()
	if id, err = deviceID(dev); err != nil {
		return "", "", err
	}
	if name, err = friendlyName(dev); err != nil {
		return "", "", err
	}
	return id, name, nil
}

// get implements `get`: "<id>\t<name>" of the default (eConsole) device.
func (s *audioSession) get() ([]byte, *exitError) {
	dev, err := s.defaultDevice(eConsole)
	if err != nil {
		return nil, err
	}
	if dev == nil {
		return nil, &exitError{code: exitNoDefault, msg: "error: no default playback device"}
	}
	defer dev.release()
	id, err := deviceID(dev)
	if err != nil {
		return nil, err
	}
	name, err := friendlyName(dev)
	if err != nil {
		return nil, err
	}
	return appendGetLine(nil, id, name), nil
}

// resolveActive looks up a user-supplied endpoint id and checks that it is in
// DEVICE_STATE_ACTIVE. GetDevice succeeds for NOTPRESENT/DISABLED/UNPLUGGED
// endpoints, so the state check is mandatory before SetDefaultEndpoint.
//
// On success it returns the canonical id as reported by IMMDevice::GetId
// (which may differ from the typed id, e.g. in letter case). That is the
// string handed to SetDefaultEndpoint and printed by `toggle`, exactly like
// resolve_active_endpoint in the C reference.
func (s *audioSession) resolveActive(id string) (string, *exitError) {
	id16, convErr := syscall.UTF16PtrFromString(id)
	if convErr != nil { // the id contains NUL, so it cannot name an endpoint
		return "", &exitError{code: exitNotActive, msg: "error: device not found: invalid id"}
	}
	var dev *comObject
	hr := s.enumerator.call(slotGetDevice, uintptr(unsafe.Pointer(id16)), uintptr(unsafe.Pointer(&dev)))
	runtime.KeepAlive(id16)
	if hr == hrNotFound || hr == hrInvalidArg {
		return "", &exitError{code: exitNotActive, msg: "error: device not found: " + id + " hr=0x" + hex8(uint32(hr))}
	}
	if hr.failed() {
		return "", comFailure("IMMDeviceEnumerator::GetDevice", hr)
	}
	defer dev.release()

	var state uint32
	if hr := dev.call(slotGetState, uintptr(unsafe.Pointer(&state))); hr.failed() {
		return "", comFailure("IMMDevice::GetState", hr)
	}
	if state != deviceStateActive {
		return "", &exitError{code: exitNotActive, msg: "error: device not active: " + id + " state=0x" + hex8(state)}
	}
	return deviceID(dev)
}

// set implements `set <id>` (and the second half of `toggle`): validate the
// endpoint, then call IPolicyConfig::SetDefaultEndpoint with its canonical id
// for eConsole, eMultimedia and eCommunications, in that order, even when it
// already is the default (the "set-noop" benchmark scenario measures exactly
// that). It returns the canonical id that was set.
func (s *audioSession) set(id string) (string, *exitError) {
	canonical, err := s.resolveActive(id)
	if err != nil {
		return "", err
	}
	canonical16, convErr := syscall.UTF16PtrFromString(canonical)
	if convErr != nil { // cannot happen: GetId never returns embedded NULs
		return "", &exitError{code: exitNotActive, msg: "error: device not found: invalid canonical id"}
	}
	defer runtime.KeepAlive(canonical16)

	policy, slot, err := newPolicyConfig()
	if err != nil {
		return "", err
	}
	defer policy.release()

	for _, role := range [...]uintptr{eConsole, eMultimedia, eCommunications} {
		if hr := policy.call(slot, uintptr(unsafe.Pointer(canonical16)), role); hr.failed() {
			return "", comFailure("IPolicyConfig::SetDefaultEndpoint(role="+strconv.Itoa(int(role))+")", hr)
		}
	}
	return canonical, nil
}

// newPolicyConfig creates IPolicyConfig (Windows 7+, SetDefaultEndpoint at
// slot 13) and falls back to IPolicyConfigVista (slot 12). It returns the
// object and the vtable slot of SetDefaultEndpoint.
func newPolicyConfig() (*comObject, int, *exitError) {
	pc, hr := coCreateInstance(&clsidPolicyConfigClient, &iidIPolicyConfig)
	if !hr.failed() {
		return pc, slotPolicyConfigSetDefaultEndpoint, nil
	}
	pcVista, hrVista := coCreateInstance(&clsidPolicyConfigVistaClient, &iidIPolicyConfigVista)
	if !hrVista.failed() {
		return pcVista, slotPolicyConfigVistaSetDefaultEndpoint, nil
	}
	return nil, 0, comFailure("CoCreateInstance(PolicyConfigClient)", hr)
}

// toggle implements `toggle <idA> <idB>`: read the eConsole default, pick the
// target with toggleTarget, then validate and set it. Prints the canonical id
// of the endpoint that was set (as IMMDevice::GetId reports it).
func (s *audioSession) toggle(idA, idB string) ([]byte, *exitError) {
	current, err := s.defaultID(eConsole) // "" when there is no default device
	if err != nil {
		return nil, err
	}
	canonical, err := s.set(toggleTarget(current, idA, idB))
	if err != nil {
		return nil, err
	}
	return append([]byte(canonical), '\n'), nil
}
