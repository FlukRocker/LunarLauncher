/**
 * Script for login.ejs
 */
// Validation Regexes.
// const validUsername         = /^[a-zA-Z0-9_]{1,16}$/
// const basicEmail            = /^\S+@\S+\.\S+$/
//const validEmail          = /^(([^<>()\[\]\.,;:\s@\"]+(\.[^<>()\[\]\.,;:\s@\"]+)*)|(\".+\"))@(([^<>()[\]\.,;:\s@\"]+\.)+[^<>()[\]\.,;:\s@\"]{2,})$/i

// Login Elements
const lunarLoginCancelContainer  = document.getElementById('lunarLoginCancelContainer')
const lunarLoginCancelButton     = document.getElementById('lunarLoginCancelButton')
const lunarLoginEmailError       = document.getElementById('lunarLoginEmailError')
const lunarLoginUsername         = document.getElementById('lunarLoginUsername')
const lunarLoginButton           = document.getElementById('lunarLoginButton')
const lunarLoginForm             = document.getElementById('lunarLoginForm')

// Control variables.

/**
 * Show a login error.
 * 
 * @param {HTMLElement} element The element on which to display the error.
 * @param {string} value The error text.
 */
function showError(element, value){
    element.innerHTML = value
    element.style.opacity = 1
}

/**
 * Shake a login error to add emphasis.
 * 
 * @param {HTMLElement} element The element to shake.
 */
function shakeError(element){
    if(element.style.opacity == 1){
        element.classList.remove('shake')
        void element.offsetWidth
        element.classList.add('shake')
    }
}

/**
 * Validate that an email field is neither empty nor invalid.
 * 
 * @param {string} value The email value.
 */
function validateEmail(value){
    if(value){
        if(!basicEmail.test(value) && !validUsername.test(value)){
            showError(lunarLoginEmailError, Lang.queryJS('login.error.invalidValue'))
            loginDisabled(true)
            lu = false
        } else {
            lunarLoginEmailError.style.opacity = 0
            lu = true
            loginDisabled(false)
        }
    } else {
        lu = false
        showError(lunarLoginEmailError, Lang.queryJS('login.error.requiredValue'))
        loginDisabled(true)
    }
}

// Emphasize errors with shake when focus is lost.
lunarLoginUsername.addEventListener('focusout', (e) => {
    validateEmail(e.target.value)
    shakeError(lunarLoginEmailError)
})

// Validate input for each field.
lunarLoginUsername.addEventListener('input', (e) => {
    validateEmail(e.target.value)
})

/**
 * Enable or disable the login button.
 * 
 * @param {boolean} v True to enable, false to disable.
 */
function loginDisabled(v){
    if(lunarLoginButton.disabled !== v){
        lunarLoginButton.disabled = v
    }
}

/**
 * Enable or disable loading elements.
 * 
 * @param {boolean} v True to enable, false to disable.
 */
function loginLoading(v){
    if(v){
        lunarLoginButton.setAttribute('loading', v)
        lunarLoginButton.innerHTML = lunarLoginButton.innerHTML.replace(Lang.queryJS('login.login'), Lang.queryJS('login.loggingIn'))
    } else {
        lunarLoginButton.removeAttribute('loading')
        lunarLoginButton.innerHTML = lunarLoginButton.innerHTML.replace(Lang.queryJS('login.loggingIn'), Lang.queryJS('login.login'))
    }
}

/**
 * Enable or disable login form.
 * 
 * @param {boolean} v True to enable, false to disable.
 */
function formDisabled(v){
    loginDisabled(v)
    lunarLoginCancelButton.disabled = v
    lunarLoginUsername.disabled = v
}

let lunarLoginViewOnSuccess = VIEWS.landing
let lunarLoginViewOnCancel = VIEWS.settings
let lunarLoginViewCancelHandler

function loginCancelEnabled(val){
    if(val){
        $(lunarLoginCancelContainer).show()
    } else {
        $(lunarLoginCancelContainer).hide()
    }
}

lunarLoginCancelButton.onclick = (e) => {
    switchView(getCurrentView(), lunarLoginViewOnCancel, 500, 500, () => {
        lunarLoginUsername.value = ''
        loginCancelEnabled(false)
        if(lunarLoginViewCancelHandler != null){
            lunarLoginViewCancelHandler()
            lunarLoginViewCancelHandler = null
        }
    })
}

// Disable default form behavior.
lunarLoginForm.onsubmit = () => { return false }

// Bind login button behavior.
lunarLoginButton.addEventListener('click', () => {
    // Disable form.
    formDisabled(true)

    // Show loading stuff.
    loginLoading(true)

    let username = lunarLoginUsername.value
    console.log(username, md5Encode(username))

    AuthManager.addLunarAccount(username).then((value) => {
        updateSelectedAccount(value)
        loginButton.innerHTML = loginButton.innerHTML.replace(Lang.queryJS('login.loggingIn'), Lang.queryJS('login.success'))
        $('.circle-loader').toggleClass('load-complete')
        $('.checkmark').toggle()
        setTimeout(() => {
            switchView(VIEWS.login, lunarLoginViewOnSuccess, 500, 500, async () => {
                // Temporary workaround
                if(lunarLoginViewOnSuccess === VIEWS.settings){
                    await prepareSettings()
                }
                $('#lunarLoginContainer').hide();
                lunarLoginViewOnSuccess = VIEWS.landing // Reset this for good measure.
                loginCancelEnabled(false) // Reset this for good measure.
                lunarLoginViewCancelHandler = null // Reset this for good measure.
                lunarLoginUsername.value = ''
                $('.circle-loader').toggleClass('load-complete')
                $('.checkmark').toggle()
                loginLoading(false)
                loginButton.innerHTML = loginButton.innerHTML.replace(Lang.queryJS('login.success'), Lang.queryJS('login.login'))
                formDisabled(false)
            })
        }, 1000)
    }).catch((displayableError) => {
        loginLoading(false)

        let actualDisplayableError
        if(isDisplayableError(displayableError)) {
            msftLoginLogger.error('Error while logging in.', displayableError)
            actualDisplayableError = displayableError
        } else {
            // Uh oh.
            msftLoginLogger.error('Unhandled error during login.', displayableError)
            actualDisplayableError = Lang.queryJS('login.error.unknown')
        }

        setOverlayContent(actualDisplayableError.title, actualDisplayableError.desc, Lang.queryJS('login.tryAgain'))
        setOverlayHandler(() => {
            formDisabled(false)
            toggleOverlay(false)
        })
        toggleOverlay(true)
    })
})