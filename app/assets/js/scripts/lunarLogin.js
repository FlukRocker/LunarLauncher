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
const lunarLoginPasswordError    = document.getElementById('lunarLoginPasswordError')
const lunarLoginPassword         = document.getElementById('lunarLoginPassword')
const lunarCheckmarkContainer    = document.getElementById('lunarCheckmarkContainer')
const lunarLoginRememberOption   = document.getElementById('lunarLoginRememberOption')
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
            if(lp){
                loginDisabled(false)
            }
        }
    } else {
        lu = false
        showError(lunarLoginEmailError, Lang.queryJS('login.error.requiredValue'))
        loginDisabled(true)
    }
}

/**
 * Validate that the password field is not empty.
 * 
 * @param {string} value The password value.
 */
function validatePassword(value){
    if(value){
        lunarLoginPasswordError.style.opacity = 0
        lp = true
        if(lu){
            loginDisabled(false)
        }
    } else {
        lp = false
        showError(lunarLoginPasswordError, Lang.queryJS('login.error.invalidValue'))
        loginDisabled(true)
    }
}

// Emphasize errors with shake when focus is lost.
lunarLoginUsername.addEventListener('focusout', (e) => {
    validateEmail(e.target.value)
    shakeError(lunarLoginEmailError)
})
lunarLoginPassword.addEventListener('focusout', (e) => {
    validatePassword(e.target.value)
    shakeError(lunarLoginPasswordError)
})

// Validate input for each field.
lunarLoginUsername.addEventListener('input', (e) => {
    validateEmail(e.target.value)
})
lunarLoginPassword.addEventListener('input', (e) => {
    validatePassword(e.target.value)
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
    lunarLoginPassword.disabled = v
    if(v){
        lunarCheckmarkContainer.setAttribute('disabled', v)
    } else {
        lunarCheckmarkContainer.removeAttribute('disabled')
    }
    lunarLoginRememberOption.disabled = v
}

// let loginViewOnSuccess = VIEWS.landing
// let loginViewOnCancel = VIEWS.settings
// let loginViewCancelHandler

function loginCancelEnabled(val){
    if(val){
        $(lunarLoginCancelContainer).show()
    } else {
        $(lunarLoginCancelContainer).hide()
    }
}

lunarLoginCancelButton.onclick = (e) => {
    switchView(getCurrentView(), loginViewOnCancel, 500, 500, () => {
        lunarLoginUsername.value = ''
        lunarLoginPassword.value = ''
        loginCancelEnabled(false)
        if(loginViewCancelHandler != null){
            loginViewCancelHandler()
            loginViewCancelHandler = null
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
    let password = lunarLoginPassword.value
    let remember = lunarLoginRememberOption.checked
    console.log(username, password, remember)
})