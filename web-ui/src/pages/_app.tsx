import Head from 'next/head'
import { Router } from 'next/router'
import type { NextPage } from 'next'
import type { AppProps } from 'next/app'
import NProgress from 'nprogress'
import { CacheProvider } from '@emotion/react'
import type { EmotionCache } from '@emotion/cache'
import themeConfig from 'src/configs/themeConfig'
import StandardVerticalLayout from 'src/layouts/StandardVerticalLayout'
import ThemeComponent from 'src/@core/theme/ThemeComponent'
import { SettingsConsumer, SettingsProvider } from 'src/@core/context/settingsContext'
import { createEmotionCache } from 'src/@core/utils/create-emotion-cache'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import 'react-perfect-scrollbar/dist/css/styles.css'
import '../../styles/globals.css'

type ExtendedAppProps = AppProps & {
  Component: NextPage
  emotionCache: EmotionCache
}

const clientSideEmotionCache = createEmotionCache()

// ** Pace Loader
if (themeConfig.routingLoader) {
  Router.events.on('routeChangeStart', () => {
    NProgress.start()
  })
  Router.events.on('routeChangeError', () => {
    NProgress.done()
  })
  Router.events.on('routeChangeComplete', () => {
    NProgress.done()
  })
}

const queryClient = new QueryClient()

const App = (props: ExtendedAppProps) => {
  const { Component, emotionCache = clientSideEmotionCache, pageProps } = props

  // Variables
  const getLayout = Component.getLayout ?? (page => <StandardVerticalLayout>{page}</StandardVerticalLayout>)

  return (
    <CacheProvider value={emotionCache}>
      <Head>
        <title>{`${themeConfig.templateName} - WebUI`}</title>
        <meta
          name='description'
          content={`${themeConfig.templateName} – WebUI.`}
        />
        <meta name='keywords' content='Database Stream Processor Configuration UI' />
        <meta name='viewport' content='initial-scale=1, width=device-width' />
      </Head>

      <SettingsProvider>
        <SettingsConsumer>
          {({ settings }) => {
            return <ThemeComponent settings={settings}>{getLayout(<QueryClientProvider client={queryClient}><Component {...pageProps} /></QueryClientProvider>)}</ThemeComponent>
          }}
        </SettingsConsumer>
      </SettingsProvider>
    </CacheProvider>
  )
}

export default App